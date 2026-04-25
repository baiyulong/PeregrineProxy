use crate::acl::auth;
use crate::config::UserCredential;
use crate::upstream::direct::DirectConnector;
use crate::upstream::{ConnectTarget, UpstreamConnector};
use bytes::Bytes;
use http_body_util::Full;
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;

pub async fn handle_http(stream: TcpStream) {
    let io = TokioIo::new(stream);

    let service = hyper::service::service_fn(|req: Request<Incoming>| async move {
        if req.method() == Method::CONNECT {
            handle_connect(req).await
        } else {
            proxy_request(req).await
        }
    });

    if let Err(e) = http1::Builder::new()
        .preserve_header_case(true)
        .title_case_headers(true)
        .serve_connection(io, service)
        .with_upgrades() // IMPORTANT: enable upgrades for CONNECT
        .await
    {
        // Don't log "connection closed" as an error
        if !e.to_string().contains("connection closed") {
            tracing::error!("HTTP connection error: {}", e);
        }
    }
}

async fn proxy_request(req: Request<Incoming>) -> Result<Response<Full<Bytes>>, hyper::Error> {
    // Parse target from absolute URI
    let uri = req.uri().clone();
    let host = match uri.host() {
        Some(h) => h.to_string(),
        None => {
            let resp = Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(Full::new(Bytes::from("Missing host in request URI")))
                .unwrap();
            return Ok(resp);
        }
    };
    let port = uri.port_u16().unwrap_or(80);

    // Connect to target
    let connector = DirectConnector;
    let target_stream = match connector
        .connect(&ConnectTarget::Address(host.clone(), port))
        .await
    {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("Failed to connect to {}:{}: {}", host, port, e);
            let resp = Response::builder()
                .status(StatusCode::BAD_GATEWAY)
                .body(Full::new(Bytes::from(format!(
                    "Failed to connect to upstream: {}",
                    e
                ))))
                .unwrap();
            return Ok(resp);
        }
    };

    // Forward request to target using hyper client
    let io = TokioIo::new(target_stream);
    let (mut sender, conn) = hyper::client::conn::http1::handshake(io).await?;

    tokio::spawn(async move {
        if let Err(e) = conn.await {
            tracing::error!("Connection to upstream failed: {}", e);
        }
    });

    // Rebuild request with relative URI and cleaned headers
    let mut builder = Request::builder()
        .method(req.method().clone())
        .uri(uri.path_and_query().map(|pq| pq.as_str()).unwrap_or("/"))
        .version(req.version());

    // Copy headers, removing hop-by-hop headers
    for (key, value) in req.headers() {
        if !is_hop_by_hop(key.as_str()) {
            builder = builder.header(key.clone(), value.clone());
        }
    }

    // Ensure Host header is set
    if !req.headers().contains_key(hyper::header::HOST) {
        builder = builder.header(hyper::header::HOST, format!("{}:{}", host, port));
    }

    let forwarded_req = builder.body(req.into_body()).unwrap();

    match sender.send_request(forwarded_req).await {
        Ok(resp) => {
            // Convert response body
            let (parts, body) = resp.into_parts();
            let body_bytes = http_body_util::BodyExt::collect(body).await?.to_bytes();
            Ok(Response::from_parts(parts, Full::new(body_bytes)))
        }
        Err(e) => {
            tracing::error!("Failed to forward request: {}", e);
            Err(e)
        }
    }
}

fn is_hop_by_hop(header: &str) -> bool {
    matches!(
        header.to_lowercase().as_str(),
        "proxy-authorization"
            | "proxy-connection"
            | "proxy-authenticate"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
            | "keep-alive"
            | "connection"
    )
}

async fn handle_connect(req: Request<Incoming>) -> Result<Response<Full<Bytes>>, hyper::Error> {
    // Extract target host:port from the CONNECT request URI
    let target = req
        .uri()
        .authority()
        .map(|a| a.to_string())
        .unwrap_or_default();

    let (host, port) = parse_host_port(&target).unwrap_or((target.clone(), 443));

    tracing::info!("CONNECT tunnel to {}:{}", host, port);

    // Spawn a task to handle the tunnel after the upgrade completes
    tokio::spawn(async move {
        match hyper::upgrade::on(req).await {
            Ok(upgraded) => {
                let mut upgraded = TokioIo::new(upgraded);

                // Connect to target
                let connector = DirectConnector;
                match connector
                    .connect(&ConnectTarget::Address(host.clone(), port))
                    .await
                {
                    Ok(mut target_stream) => {
                        // Bidirectional copy
                        match tokio::io::copy_bidirectional(&mut upgraded, &mut target_stream).await
                        {
                            Ok((from_client, from_server)) => {
                                tracing::debug!(
                                    "CONNECT tunnel closed: {} bytes from client, {} bytes from server",
                                    from_client, from_server
                                );
                            }
                            Err(e) => {
                                tracing::debug!("CONNECT tunnel error: {}", e);
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!("Failed to connect to {}:{}: {}", host, port, e);
                    }
                }
            }
            Err(e) => {
                tracing::error!("Upgrade failed: {}", e);
            }
        }
    });

    // Return 200 to signal tunnel is established
    Ok(Response::new(Full::new(Bytes::new())))
}

fn parse_host_port(authority: &str) -> Option<(String, u16)> {
    if let Some(colon_pos) = authority.rfind(':') {
        let host = &authority[..colon_pos];
        let port = authority[colon_pos + 1..].parse::<u16>().ok()?;
        Some((host.to_string(), port))
    } else {
        None
    }
}

/// Check Proxy-Authorization header and verify credentials
/// Returns true if credentials are valid, false otherwise
pub fn check_proxy_auth<T>(req: &Request<T>, users: &[UserCredential]) -> bool {
    // Look for Proxy-Authorization header
    let auth_header = match req.headers().get("proxy-authorization") {
        Some(header_value) => header_value,
        None => return false, // No auth header provided
    };

    // Convert header value to string
    let auth_str = match auth_header.to_str() {
        Ok(s) => s,
        Err(_) => return false, // Invalid header value
    };

    // Parse Basic Auth
    let (username, password) = match auth::parse_basic_auth(auth_str) {
        Some(credentials) => credentials,
        None => return false, // Invalid Basic Auth format
    };

    // Verify credentials
    auth::verify_credentials(&username, &password, users)
}

/// Create 407 Proxy Authentication Required response
pub fn make_407_response() -> Response<Full<Bytes>> {
    Response::builder()
        .status(StatusCode::PROXY_AUTHENTICATION_REQUIRED)
        .header("Proxy-Authenticate", "Basic realm=\"Peregrine Proxy\"")
        .body(Full::new(Bytes::from("Proxy Authentication Required")))
        .unwrap()
}

/// Generic HTTP handler that works with any AsyncRead + AsyncWrite + Unpin stream
pub async fn handle_http_generic<S>(stream: S)
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let io = TokioIo::new(stream);

    let service = hyper::service::service_fn(|req: Request<Incoming>| async move {
        if req.method() == Method::CONNECT {
            handle_connect(req).await
        } else {
            proxy_request(req).await
        }
    });

    if let Err(e) = http1::Builder::new()
        .preserve_header_case(true)
        .title_case_headers(true)
        .serve_connection(io, service)
        .with_upgrades() // IMPORTANT: enable upgrades for CONNECT
        .await
    {
        // Don't log "connection closed" as an error
        if !e.to_string().contains("connection closed") {
            tracing::error!("HTTP connection error: {}", e);
        }
    }
}
