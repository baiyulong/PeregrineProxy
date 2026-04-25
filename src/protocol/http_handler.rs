use hyper::{Request, Response, Method, StatusCode};
use hyper::body::Incoming;
use http_body_util::Full;
use bytes::Bytes;
use hyper::server::conn::http1;
use hyper_util::rt::TokioIo;
use tokio::net::TcpStream;
use crate::upstream::direct::DirectConnector;
use crate::upstream::{ConnectTarget, UpstreamConnector};

pub async fn handle_http(stream: TcpStream) {
    let io = TokioIo::new(stream);
    
    let service = hyper::service::service_fn(|req: Request<Incoming>| async move {
        proxy_request(req).await
    });
    
    if let Err(e) = http1::Builder::new()
        .preserve_header_case(true)
        .title_case_headers(true)
        .serve_connection(io, service)
        .await
    {
        tracing::error!("HTTP connection error: {}", e);
    }
}

async fn proxy_request(req: Request<Incoming>) -> Result<Response<Full<Bytes>>, hyper::Error> {
    // For CONNECT, return 405 for now (Task 7)
    if req.method() == Method::CONNECT {
        let resp = Response::builder()
            .status(StatusCode::METHOD_NOT_ALLOWED)
            .body(Full::new(Bytes::from("CONNECT not yet supported")))
            .unwrap();
        return Ok(resp);
    }
    
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
    let target_stream = match connector.connect(&ConnectTarget::Address(host.clone(), port)).await {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("Failed to connect to {}:{}: {}", host, port, e);
            let resp = Response::builder()
                .status(StatusCode::BAD_GATEWAY)
                .body(Full::new(Bytes::from(format!("Failed to connect to upstream: {}", e))))
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
