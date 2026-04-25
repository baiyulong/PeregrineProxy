use crate::config::{UpstreamConfig, UserCredential};
use crate::error::{ProxyError, ProxyResult};
use crate::upstream::{BoxedStream, ConnectTarget, UpstreamConnector};
use async_trait::async_trait;
use base64::Engine;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// Chain connector that connects through multiple proxies sequentially
pub struct ChainConnector {
    pub chain: Vec<UpstreamConfig>,
}

#[async_trait]
impl UpstreamConnector for ChainConnector {
    async fn connect(&self, target: &ConnectTarget) -> ProxyResult<BoxedStream> {
        if self.chain.is_empty() {
            return Err(ProxyError::Upstream("Empty chain".into()));
        }

        if self.chain.len() == 1 {
            // Single hop chain - handle directly based on config type
            return match &self.chain[0] {
                UpstreamConfig::Direct => {
                    // Direct connection to target
                    match target {
                        ConnectTarget::Address(host, port) => {
                            let stream = TcpStream::connect(format!("{}:{}", host, port))
                                .await
                                .map_err(|e| {
                                    ProxyError::Upstream(format!("Direct connection failed: {}", e))
                                })?;
                            Ok(Box::pin(stream) as BoxedStream)
                        }
                    }
                }
                config => {
                    // Single proxy hop - use existing connector logic
                    let proxy_addr = extract_proxy_address(config)?;
                    let stream = TcpStream::connect(&proxy_addr).await.map_err(|e| {
                        ProxyError::Upstream(format!(
                            "Failed to connect to proxy {}: {}",
                            proxy_addr, e
                        ))
                    })?;
                    let boxed_stream: BoxedStream = Box::pin(stream);
                    perform_proxy_handshake(boxed_stream, config, target).await
                }
            };
        }

        // Multi-hop chain
        // Step 1: Connect to first proxy
        let first_proxy_addr = extract_proxy_address(&self.chain[0])?;
        let stream = TcpStream::connect(&first_proxy_addr).await.map_err(|e| {
            ProxyError::Upstream(format!(
                "Failed to connect to first proxy {}: {}",
                first_proxy_addr, e
            ))
        })?;
        let mut boxed_stream: BoxedStream = Box::pin(stream);

        // Step 2: For each hop in the chain, perform proxy handshake
        for i in 0..self.chain.len() {
            let next_target = if i + 1 < self.chain.len() {
                // Connect to next proxy's address (or target if next is Direct)
                match &self.chain[i + 1] {
                    UpstreamConfig::Direct => {
                        // Next hop is direct - connect to final target
                        target.clone()
                    }
                    next_config => {
                        // Next hop is a proxy - connect to its address
                        let next_addr = extract_proxy_address(next_config)?;
                        parse_addr_to_target(&next_addr)?
                    }
                }
            } else {
                // Last hop connects to the actual target
                target.clone()
            };

            boxed_stream =
                perform_proxy_handshake(boxed_stream, &self.chain[i], &next_target).await?;
        }

        Ok(boxed_stream)
    }
}

/// Extract proxy address from upstream config
fn extract_proxy_address(config: &UpstreamConfig) -> ProxyResult<String> {
    match config {
        UpstreamConfig::Direct => Err(ProxyError::Upstream(
            "Direct config has no proxy address".into(),
        )),
        UpstreamConfig::Http { addr, .. } => Ok(addr.clone()),
        UpstreamConfig::Socks5 { addr, .. } => Ok(addr.clone()),
        UpstreamConfig::Chain { .. } => {
            Err(ProxyError::Upstream("Nested chains not supported".into()))
        }
    }
}

/// Parse "host:port" string into ConnectTarget
fn parse_addr_to_target(addr: &str) -> ProxyResult<ConnectTarget> {
    if let Some(colon_pos) = addr.rfind(':') {
        let host = addr[..colon_pos].to_string();
        let port_str = &addr[colon_pos + 1..];
        let port = port_str
            .parse::<u16>()
            .map_err(|_| ProxyError::Upstream(format!("Invalid port in address: {}", addr)))?;
        Ok(ConnectTarget::Address(host, port))
    } else {
        Err(ProxyError::Upstream(format!(
            "Address missing port: {}",
            addr
        )))
    }
}

/// Perform proxy handshake over existing stream
async fn perform_proxy_handshake(
    stream: BoxedStream,
    config: &UpstreamConfig,
    target: &ConnectTarget,
) -> ProxyResult<BoxedStream> {
    match config {
        UpstreamConfig::Direct => {
            // Direct connector: just pass through the stream
            Ok(stream)
        }
        UpstreamConfig::Http { auth, tls, .. } => {
            perform_http_handshake(stream, auth, *tls, target).await
        }
        UpstreamConfig::Socks5 { auth, tls, .. } => {
            perform_socks5_handshake(stream, auth, *tls, target).await
        }
        UpstreamConfig::Chain { .. } => {
            Err(ProxyError::Upstream("Nested chains not supported".into()))
        }
    }
}

/// Perform HTTP CONNECT handshake over existing stream
async fn perform_http_handshake(
    mut stream: BoxedStream,
    auth: &Option<UserCredential>,
    use_tls: Option<bool>,
    target: &ConnectTarget,
) -> ProxyResult<BoxedStream> {
    // Note: For chaining, TLS should be handled at the first hop level
    // Here we assume we're working over an already established stream
    let _use_tls = use_tls.unwrap_or(false);

    match target {
        ConnectTarget::Address(host, port) => {
            // Send CONNECT request
            let mut request = format!(
                "CONNECT {}:{} HTTP/1.1\r\nHost: {}:{}\r\n",
                host, port, host, port
            );

            // Add proxy auth if configured
            if let Some(ref auth) = auth {
                let credentials = base64::engine::general_purpose::STANDARD
                    .encode(format!("{}:{}", auth.username, auth.password));
                request.push_str(&format!("Proxy-Authorization: Basic {}\r\n", credentials));
            }

            request.push_str("\r\n");

            stream
                .write_all(request.as_bytes())
                .await
                .map_err(ProxyError::Io)?;

            // Read response
            let mut buf = vec![0u8; 4096];
            let n = stream.read(&mut buf).await.map_err(ProxyError::Io)?;
            let response = String::from_utf8_lossy(&buf[..n]);

            if !response.starts_with("HTTP/1.1 200") && !response.starts_with("HTTP/1.0 200") {
                return Err(ProxyError::Upstream(format!(
                    "HTTP proxy CONNECT failed: {}",
                    response.lines().next().unwrap_or("empty")
                )));
            }

            Ok(stream)
        }
    }
}

/// Perform SOCKS5 handshake over existing stream
async fn perform_socks5_handshake(
    mut stream: BoxedStream,
    auth: &Option<UserCredential>,
    use_tls: Option<bool>,
    target: &ConnectTarget,
) -> ProxyResult<BoxedStream> {
    // Note: For chaining, TLS should be handled at the first hop level
    let _use_tls = use_tls.unwrap_or(false);

    // Phase 1: Method negotiation
    let methods = if auth.is_some() {
        vec![0x05, 0x02, 0x00, 0x02] // VER, NMETHODS=2, NO_AUTH + USERNAME_PASSWORD
    } else {
        vec![0x05, 0x01, 0x00] // VER, NMETHODS=1, NO_AUTH
    };
    stream.write_all(&methods).await.map_err(ProxyError::Io)?;

    let mut resp = [0u8; 2];
    stream.read_exact(&mut resp).await.map_err(ProxyError::Io)?;

    if resp[0] != 0x05 {
        return Err(ProxyError::Socks5(
            "Invalid SOCKS5 version from upstream".into(),
        ));
    }

    match resp[1] {
        0x00 => {} // No auth required
        0x02 => {
            // Username/password auth (RFC 1929)
            let auth = auth.as_ref().ok_or_else(|| {
                ProxyError::Socks5("Upstream requires auth but no credentials configured".into())
            })?;

            let mut auth_req = vec![0x01]; // VER
            auth_req.push(auth.username.len() as u8);
            auth_req.extend_from_slice(auth.username.as_bytes());
            auth_req.push(auth.password.len() as u8);
            auth_req.extend_from_slice(auth.password.as_bytes());
            stream.write_all(&auth_req).await.map_err(ProxyError::Io)?;

            let mut auth_resp = [0u8; 2];
            stream
                .read_exact(&mut auth_resp)
                .await
                .map_err(ProxyError::Io)?;

            if auth_resp[1] != 0x00 {
                return Err(ProxyError::AuthFailed);
            }
        }
        0xFF => {
            return Err(ProxyError::Socks5(
                "Upstream SOCKS5 proxy rejected all auth methods".into(),
            ))
        }
        other => {
            return Err(ProxyError::Socks5(format!(
                "Unsupported auth method from upstream: {}",
                other
            )))
        }
    }

    // Phase 2: CONNECT request
    match target {
        ConnectTarget::Address(host, port) => {
            let mut req = vec![0x05, 0x01, 0x00]; // VER, CMD=CONNECT, RSV

            // Try to parse as IP, otherwise use domain
            if let Ok(ipv4) = host.parse::<std::net::Ipv4Addr>() {
                req.push(0x01); // ATYP = IPv4
                req.extend_from_slice(&ipv4.octets());
            } else if let Ok(ipv6) = host.parse::<std::net::Ipv6Addr>() {
                req.push(0x04); // ATYP = IPv6
                req.extend_from_slice(&ipv6.octets());
            } else {
                req.push(0x03); // ATYP = Domain
                req.push(host.len() as u8);
                req.extend_from_slice(host.as_bytes());
            }
            req.extend_from_slice(&port.to_be_bytes());

            stream.write_all(&req).await.map_err(ProxyError::Io)?;

            // Read reply header: VER | REP | RSV | ATYP
            let mut reply_header = [0u8; 4];
            stream
                .read_exact(&mut reply_header)
                .await
                .map_err(ProxyError::Io)?;

            if reply_header[1] != 0x00 {
                let err_msg = match reply_header[1] {
                    0x01 => "general SOCKS server failure",
                    0x02 => "connection not allowed by ruleset",
                    0x03 => "network unreachable",
                    0x04 => "host unreachable",
                    0x05 => "connection refused",
                    0x06 => "TTL expired",
                    0x07 => "command not supported",
                    0x08 => "address type not supported",
                    _ => "unknown error",
                };
                return Err(ProxyError::Upstream(format!(
                    "SOCKS5 upstream CONNECT failed: {}",
                    err_msg
                )));
            }

            // Read and discard bind address based on ATYP
            match reply_header[3] {
                0x01 => {
                    // IPv4: 4 bytes + 2 port
                    let mut buf = [0u8; 6];
                    stream.read_exact(&mut buf).await.map_err(ProxyError::Io)?;
                }
                0x03 => {
                    // Domain: 1 len + domain + 2 port
                    let mut len = [0u8; 1];
                    stream.read_exact(&mut len).await.map_err(ProxyError::Io)?;
                    let mut buf = vec![0u8; len[0] as usize + 2];
                    stream.read_exact(&mut buf).await.map_err(ProxyError::Io)?;
                }
                0x04 => {
                    // IPv6: 16 bytes + 2 port
                    let mut buf = [0u8; 18];
                    stream.read_exact(&mut buf).await.map_err(ProxyError::Io)?;
                }
                _ => {}
            }

            Ok(stream)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::UpstreamConfig;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// Mock echo server
    async fn start_echo_server() -> std::net::SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            loop {
                let (mut stream, _) = listener.accept().await.unwrap();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 4096];
                    loop {
                        match stream.read(&mut buf).await {
                            Ok(0) => break,
                            Ok(n) => {
                                stream.write_all(&buf[..n]).await.unwrap();
                            }
                            Err(_) => break,
                        }
                    }
                });
            }
        });

        addr
    }

    /// Mock HTTP proxy that handles CONNECT requests
    async fn start_mock_http_proxy() -> std::net::SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            loop {
                let (mut stream, _) = listener.accept().await.unwrap();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 4096];
                    let n = stream.read(&mut buf).await.unwrap();
                    let request = String::from_utf8_lossy(&buf[..n]);

                    if request.starts_with("CONNECT") {
                        // Parse target from CONNECT request
                        let target = request.split_whitespace().nth(1).unwrap_or("");
                        let parts: Vec<&str> = target.split(':').collect();
                        let host = parts[0];
                        let port: u16 = parts.get(1).and_then(|p| p.parse().ok()).unwrap_or(80);

                        // Connect to target
                        match tokio::net::TcpStream::connect(format!("{}:{}", host, port)).await {
                            Ok(mut target_stream) => {
                                // Send 200 OK
                                stream
                                    .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
                                    .await
                                    .unwrap();
                                // Bidirectional copy
                                let _ =
                                    tokio::io::copy_bidirectional(&mut stream, &mut target_stream)
                                        .await;
                            }
                            Err(_) => {
                                stream
                                    .write_all(b"HTTP/1.1 502 Bad Gateway\r\n\r\n")
                                    .await
                                    .unwrap();
                            }
                        }
                    }
                });
            }
        });

        addr
    }

    #[tokio::test]
    async fn test_chain_connector_empty_chain_fails() {
        let connector = ChainConnector { chain: vec![] };

        let echo_addr = start_echo_server().await;
        let result = connector
            .connect(&ConnectTarget::Address(
                "127.0.0.1".into(),
                echo_addr.port(),
            ))
            .await;

        assert!(result.is_err());
        if let Err(e) = result {
            assert!(e.to_string().contains("Empty chain"));
        }
    }

    #[tokio::test]
    async fn test_chain_connector_single_direct_hop() {
        let echo_addr = start_echo_server().await;

        let connector = ChainConnector {
            chain: vec![UpstreamConfig::Direct],
        };

        let mut stream = connector
            .connect(&ConnectTarget::Address(
                "127.0.0.1".into(),
                echo_addr.port(),
            ))
            .await
            .unwrap();

        // Send data through the chain
        use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
        stream
            .write_all(b"Hello via chain (direct)!")
            .await
            .unwrap();

        let mut buf = vec![0u8; 4096];
        let n = stream.read(&mut buf).await.unwrap();
        assert_eq!(&buf[..n], b"Hello via chain (direct)!");
    }

    #[tokio::test]
    async fn test_chain_connector_two_hop_http_to_direct() {
        let echo_addr = start_echo_server().await;
        let http_proxy_addr = start_mock_http_proxy().await;

        // Chain: HTTP proxy -> Direct to target
        let connector = ChainConnector {
            chain: vec![
                UpstreamConfig::Http {
                    addr: http_proxy_addr.to_string(),
                    auth: None,
                    tls: None,
                },
                UpstreamConfig::Direct,
            ],
        };

        let mut stream = connector
            .connect(&ConnectTarget::Address(
                "127.0.0.1".into(),
                echo_addr.port(),
            ))
            .await
            .unwrap();

        use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
        stream.write_all(b"Hello via two-hop chain!").await.unwrap();

        let mut buf = vec![0u8; 4096];
        let n = stream.read(&mut buf).await.unwrap();
        assert_eq!(&buf[..n], b"Hello via two-hop chain!");
    }

    #[tokio::test]
    async fn test_chain_connector_three_hop() {
        let echo_addr = start_echo_server().await;
        let http_proxy1_addr = start_mock_http_proxy().await;
        let http_proxy2_addr = start_mock_http_proxy().await;

        // Chain: HTTP proxy1 -> HTTP proxy2 -> Direct to target
        let connector = ChainConnector {
            chain: vec![
                UpstreamConfig::Http {
                    addr: http_proxy1_addr.to_string(),
                    auth: None,
                    tls: None,
                },
                UpstreamConfig::Http {
                    addr: http_proxy2_addr.to_string(),
                    auth: None,
                    tls: None,
                },
                UpstreamConfig::Direct,
            ],
        };

        let mut stream = connector
            .connect(&ConnectTarget::Address(
                "127.0.0.1".into(),
                echo_addr.port(),
            ))
            .await
            .unwrap();

        use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
        stream
            .write_all(b"Hello via three-hop chain!")
            .await
            .unwrap();

        let mut buf = vec![0u8; 4096];
        let n = stream.read(&mut buf).await.unwrap();
        assert_eq!(&buf[..n], b"Hello via three-hop chain!");
    }
}
