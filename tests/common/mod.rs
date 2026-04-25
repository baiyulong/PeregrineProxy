use std::net::{SocketAddr, Ipv4Addr};
use tokio::net::{TcpListener, TcpStream};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use std::time::Duration;

/// Mock HTTP server that accepts requests and returns a known response
pub async fn start_mock_http_server() -> (SocketAddr, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    
    let handle = tokio::spawn(async move {
        loop {
            if let Ok((mut stream, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 4096];
                    if stream.read(&mut buf).await.is_ok() {
                        let response = "HTTP/1.1 200 OK\r\nContent-Length: 21\r\nConnection: close\r\n\r\nHello from mock server";
                        let _ = stream.write_all(response.as_bytes()).await;
                    }
                });
            }
        }
    });
    
    (addr, handle)
}

/// Mock TCP echo server for CONNECT tunnel testing
pub async fn start_echo_server() -> (SocketAddr, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    
    let handle = tokio::spawn(async move {
        loop {
            if let Ok((mut stream, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 4096];
                    loop {
                        match stream.read(&mut buf).await {
                            Ok(0) => break, // Connection closed
                            Ok(n) => {
                                if stream.write_all(&buf[..n]).await.is_err() {
                                    break;
                                }
                            }
                            Err(_) => break,
                        }
                    }
                });
            }
        }
    });
    
    (addr, handle)
}

/// Start a mock upstream HTTP proxy server
pub async fn start_mock_http_proxy() -> (SocketAddr, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    
    let handle = tokio::spawn(async move {
        loop {
            if let Ok((mut client_stream, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 4096];
                    if let Ok(n) = client_stream.read(&mut buf).await {
                        let request = String::from_utf8_lossy(&buf[..n]);
                        
                        if request.starts_with("CONNECT") {
                            // Handle CONNECT tunnel
                            let response = "HTTP/1.1 200 Connection established\r\n\r\n";
                            if client_stream.write_all(response.as_bytes()).await.is_ok() {
                                // Extract target from CONNECT request
                                if let Some(target_line) = request.lines().next() {
                                    if let Some(target) = target_line.split_whitespace().nth(1) {
                                        // Connect to actual target
                                        if let Ok(mut target_stream) = TcpStream::connect(target).await {
                                            // Relay data between client and target
                                            let (mut client_read, mut client_write) = client_stream.split();
                                            let (mut target_read, mut target_write) = target_stream.split();
                                            
                                            tokio::select! {
                                                _ = tokio::io::copy(&mut client_read, &mut target_write) => {},
                                                _ = tokio::io::copy(&mut target_read, &mut client_write) => {},
                                            }
                                        }
                                    }
                                }
                            }
                        } else {
                            // Handle regular HTTP request - just forward to target
                            // Extract URL from request line
                            if let Some(first_line) = request.lines().next() {
                                if let Some(url_part) = first_line.split_whitespace().nth(1) {
                                    if let Some(url_without_scheme) = url_part.strip_prefix("http://") {
                                        // Parse target host and port
                                        let mut parts = url_without_scheme.split('/');
                                        if let Some(host_port) = parts.next() {
                                            let target = if host_port.contains(':') {
                                                host_port.to_string()
                                            } else {
                                                format!("{}:80", host_port)
                                            };
                                            
                                            if let Ok(mut target_stream) = TcpStream::connect(target).await {
                                                // Forward the request to target
                                                let _ = target_stream.write_all(&buf[..n]).await;
                                                
                                                // Relay response back
                                                let mut response_buf = vec![0u8; 4096];
                                                if let Ok(resp_n) = target_stream.read(&mut response_buf).await {
                                                    let _ = client_stream.write_all(&response_buf[..resp_n]).await;
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                });
            }
        }
    });
    
    (addr, handle)
}

/// Start a mock upstream SOCKS5 proxy server
pub async fn start_mock_socks5_proxy() -> (SocketAddr, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    
    let handle = tokio::spawn(async move {
        loop {
            if let Ok((mut stream, _)) = listener.accept().await {
                tokio::spawn(async move {
                    // Simple SOCKS5 handshake
                    let mut buf = [0u8; 256];
                    
                    // Method negotiation
                    if let Ok(n) = stream.read(&mut buf).await {
                        if n >= 3 && buf[0] == 0x05 { // SOCKS5
                            // Accept no-auth method
                            let _ = stream.write_all(&[0x05, 0x00]).await;
                            
                            // Read CONNECT request
                            if let Ok(n) = stream.read(&mut buf).await {
                                if n >= 10 && buf[1] == 0x01 { // CONNECT command
                                    // Parse target address
                                    let atyp = buf[3];
                                    let target = match atyp {
                                        0x01 => { // IPv4
                                            let ip = Ipv4Addr::new(buf[4], buf[5], buf[6], buf[7]);
                                            let port = u16::from_be_bytes([buf[8], buf[9]]);
                                            format!("{}:{}", ip, port)
                                        }
                                        0x03 => { // Domain name
                                            let domain_len = buf[4] as usize;
                                            let domain = String::from_utf8_lossy(&buf[5..5 + domain_len]);
                                            let port_start = 5 + domain_len;
                                            let port = u16::from_be_bytes([buf[port_start], buf[port_start + 1]]);
                                            format!("{}:{}", domain, port)
                                        }
                                        _ => return,
                                    };
                                    
                                    // Connect to target
                                    if let Ok(mut target_stream) = TcpStream::connect(target).await {
                                        // Send success response
                                        let response = [
                                            0x05, 0x00, 0x00, 0x01, // VER, REP=success, RSV, ATYP=IPv4
                                            127, 0, 0, 1, // Bind IP
                                            0x00, 0x00, // Bind port
                                        ];
                                        if stream.write_all(&response).await.is_ok() {
                                            // Relay data
                                            let (mut client_read, mut client_write) = stream.split();
                                            let (mut target_read, mut target_write) = target_stream.split();
                                            
                                            tokio::select! {
                                                _ = tokio::io::copy(&mut client_read, &mut target_write) => {},
                                                _ = tokio::io::copy(&mut target_read, &mut client_write) => {},
                                            }
                                        }
                                    } else {
                                        // Send connection refused
                                        let response = [
                                            0x05, 0x05, 0x00, 0x01, // VER, REP=connection refused
                                            0, 0, 0, 0, 0x00, 0x00,
                                        ];
                                        let _ = stream.write_all(&response).await;
                                    }
                                }
                            }
                        }
                    }
                });
            }
        }
    });
    
    (addr, handle)
}

/// Helper to send HTTP request through proxy
pub async fn http_request_through_proxy(
    proxy_addr: SocketAddr,
    target_url: &str,
) -> tokio::io::Result<String> {
    let mut stream = TcpStream::connect(proxy_addr).await?;
    
    let request = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
        target_url,
        target_url.trim_start_matches("http://").split('/').next().unwrap_or("localhost")
    );
    
    stream.write_all(request.as_bytes()).await?;
    
    let mut response = vec![0u8; 4096];
    let n = tokio::time::timeout(Duration::from_secs(5), stream.read(&mut response))
        .await??;
    
    Ok(String::from_utf8_lossy(&response[..n]).to_string())
}

/// Helper to do CONNECT tunnel through proxy
pub async fn connect_tunnel_through_proxy(
    proxy_addr: SocketAddr,
    target_host: &str,
    target_port: u16,
) -> tokio::io::Result<TcpStream> {
    let mut stream = TcpStream::connect(proxy_addr).await?;
    
    let connect_req = format!(
        "CONNECT {}:{} HTTP/1.1\r\nHost: {}:{}\r\n\r\n",
        target_host, target_port, target_host, target_port
    );
    
    stream.write_all(connect_req.as_bytes()).await?;
    
    // Read 200 response
    let mut response = vec![0u8; 4096];
    let n = tokio::time::timeout(Duration::from_secs(5), stream.read(&mut response))
        .await??;
    let response_str = String::from_utf8_lossy(&response[..n]);
    
    if response_str.contains("200") {
        Ok(stream)
    } else {
        Err(tokio::io::Error::other(
            format!("CONNECT failed: {}", response_str)
        ))
    }
}

/// Helper to connect through SOCKS5 proxy
pub async fn socks5_connect_through_proxy(
    proxy_addr: SocketAddr,
    target_host: &str,
    target_port: u16,
) -> tokio::io::Result<TcpStream> {
    let mut stream = TcpStream::connect(proxy_addr).await?;
    
    // Method negotiation (no auth)
    stream.write_all(&[0x05, 0x01, 0x00]).await?;
    
    let mut resp = [0u8; 2];
    tokio::time::timeout(Duration::from_secs(5), stream.read_exact(&mut resp))
        .await??;
    
    if resp != [0x05, 0x00] {
        return Err(tokio::io::Error::other(
            "SOCKS5 method negotiation failed"
        ));
    }
    
    // CONNECT request with domain name
    let domain_bytes = target_host.as_bytes();
    let mut connect_req = vec![0x05, 0x01, 0x00, 0x03]; // VER, CMD=CONNECT, RSV, ATYP=Domain
    connect_req.push(domain_bytes.len() as u8);
    connect_req.extend_from_slice(domain_bytes);
    connect_req.extend_from_slice(&target_port.to_be_bytes());
    
    stream.write_all(&connect_req).await?;
    
    // Read reply
    let mut reply = [0u8; 10]; // Assume IPv4 reply format
    tokio::time::timeout(Duration::from_secs(5), stream.read(&mut reply))
        .await??;
    
    if reply[0] != 0x05 || reply[1] != 0x00 {
        return Err(tokio::io::Error::other(
            format!("SOCKS5 CONNECT failed: reply[1]={}", reply[1])
        ));
    }
    
    Ok(stream)
}

/// Helper to start HTTP proxy server
pub async fn start_http_proxy_server() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    
    tokio::spawn(async move {
        loop {
            if let Ok((stream, _)) = listener.accept().await {
                tokio::spawn(async move {
                    peregrine::protocol::http_handler::handle_http(stream).await;
                });
            }
        }
    });
    
    addr
}

/// Helper to start SOCKS5 proxy server
pub async fn start_socks5_proxy_server() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    
    tokio::spawn(async move {
        loop {
            if let Ok((stream, peer_addr)) = listener.accept().await {
                tokio::spawn(async move {
                    peregrine::protocol::socks5::handle_socks5(stream, peer_addr, None).await;
                });
            }
        }
    });
    
    addr
}

/// Helper to start SOCKS5 proxy server with authentication
pub async fn start_socks5_proxy_server_with_auth(users: Vec<peregrine::config::UserCredential>) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    
    tokio::spawn(async move {
        loop {
            if let Ok((stream, peer_addr)) = listener.accept().await {
                let users = Some(users.clone());
                tokio::spawn(async move {
                    peregrine::protocol::socks5::handle_socks5(stream, peer_addr, users).await;
                });
            }
        }
    });
    
    addr
}