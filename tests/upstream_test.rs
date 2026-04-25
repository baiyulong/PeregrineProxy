use tokio::net::TcpListener;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

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
                            stream.write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n").await.unwrap();
                            // Bidirectional copy
                            let _ = tokio::io::copy_bidirectional(&mut stream, &mut target_stream).await;
                        }
                        Err(_) => {
                            stream.write_all(b"HTTP/1.1 502 Bad Gateway\r\n\r\n").await.unwrap();
                        }
                    }
                }
            });
        }
    });
    
    addr
}

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
                        Ok(n) => { stream.write_all(&buf[..n]).await.unwrap(); }
                        Err(_) => break,
                    }
                }
            });
        }
    });
    
    addr
}

#[tokio::test]
async fn test_http_proxy_connector_connect() {
    use peregrine::upstream::http_proxy::HttpProxyConnector;
    use peregrine::upstream::{ConnectTarget, UpstreamConnector};
    
    let echo_addr = start_echo_server().await;
    let proxy_addr = start_mock_http_proxy().await;
    
    let connector = HttpProxyConnector {
        proxy_addr: proxy_addr.to_string(),
        auth: None,
    };
    
    let mut stream = connector.connect(&ConnectTarget::Address(
        "127.0.0.1".into(), echo_addr.port()
    )).await.unwrap();
    
    // Send data through the tunnel
    use tokio::io::{AsyncWriteExt as _, AsyncReadExt as _};
    stream.write_all(b"Hello via HTTP proxy!").await.unwrap();
    
    let mut buf = vec![0u8; 4096];
    let n = stream.read(&mut buf).await.unwrap();
    assert_eq!(&buf[..n], b"Hello via HTTP proxy!");
}

#[tokio::test]
async fn test_direct_connector() {
    use peregrine::upstream::direct::DirectConnector;
    use peregrine::upstream::{ConnectTarget, UpstreamConnector};
    
    let echo_addr = start_echo_server().await;
    
    let connector = DirectConnector;
    let mut stream = connector.connect(&ConnectTarget::Address(
        "127.0.0.1".into(), echo_addr.port()
    )).await.unwrap();
    
    use tokio::io::{AsyncWriteExt as _, AsyncReadExt as _};
    stream.write_all(b"Direct hello!").await.unwrap();
    
    let mut buf = vec![0u8; 4096];
    let n = stream.read(&mut buf).await.unwrap();
    assert_eq!(&buf[..n], b"Direct hello!");
}

#[tokio::test]
async fn test_upstream_router_direct() {
    use peregrine::upstream::UpstreamRouter;
    use peregrine::upstream::{ConnectTarget, UpstreamConnector};
    use peregrine::config::UpstreamConfig;
    
    let echo_addr = start_echo_server().await;
    
    let router = UpstreamRouter::from_config(&UpstreamConfig::Direct);
    let mut stream = router.connector().connect(&ConnectTarget::Address(
        "127.0.0.1".into(), echo_addr.port()
    )).await.unwrap();
    
    use tokio::io::{AsyncWriteExt as _, AsyncReadExt as _};
    stream.write_all(b"Router direct!").await.unwrap();
    
    let mut buf = vec![0u8; 4096];
    let n = stream.read(&mut buf).await.unwrap();
    assert_eq!(&buf[..n], b"Router direct!");
}

#[tokio::test]
async fn test_upstream_router_http_proxy() {
    use peregrine::upstream::UpstreamRouter;
    use peregrine::upstream::{ConnectTarget, UpstreamConnector};
    use peregrine::config::UpstreamConfig;
    
    let echo_addr = start_echo_server().await;
    let proxy_addr = start_mock_http_proxy().await;
    
    let router = UpstreamRouter::from_config(&UpstreamConfig::Http {
        addr: proxy_addr.to_string(),
        auth: None,
    });
    let mut stream = router.connector().connect(&ConnectTarget::Address(
        "127.0.0.1".into(), echo_addr.port()
    )).await.unwrap();
    
    use tokio::io::{AsyncWriteExt as _, AsyncReadExt as _};
    stream.write_all(b"Router via proxy!").await.unwrap();
    
    let mut buf = vec![0u8; 4096];
    let n = stream.read(&mut buf).await.unwrap();
    assert_eq!(&buf[..n], b"Router via proxy!");
}

/// Start a SOCKS5 proxy (reusing our own implementation)
async fn start_socks5_proxy() -> std::net::SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    
    tokio::spawn(async move {
        loop {
            let (stream, peer_addr) = listener.accept().await.unwrap();
            tokio::spawn(async move {
                peregrine::protocol::socks5::handle_socks5(stream, peer_addr).await;
            });
        }
    });
    
    addr
}

#[tokio::test]
async fn test_socks5_proxy_connector() {
    use peregrine::upstream::socks5_proxy::Socks5ProxyConnector;
    use peregrine::upstream::{ConnectTarget, UpstreamConnector};
    
    let echo_addr = start_echo_server().await;
    let proxy_addr = start_socks5_proxy().await;
    
    let connector = Socks5ProxyConnector {
        proxy_addr: proxy_addr.to_string(),
        auth: None,
    };
    
    let mut stream = connector.connect(&ConnectTarget::Address(
        "127.0.0.1".into(), echo_addr.port()
    )).await.unwrap();
    
    use tokio::io::{AsyncWriteExt as _, AsyncReadExt as _};
    stream.write_all(b"Hello via SOCKS5 proxy!").await.unwrap();
    
    let mut buf = vec![0u8; 4096];
    let n = stream.read(&mut buf).await.unwrap();
    assert_eq!(&buf[..n], b"Hello via SOCKS5 proxy!");
}

#[tokio::test]
async fn test_upstream_router_socks5() {
    use peregrine::upstream::UpstreamRouter;
    use peregrine::upstream::{ConnectTarget, UpstreamConnector};
    use peregrine::config::UpstreamConfig;
    
    let echo_addr = start_echo_server().await;
    let proxy_addr = start_socks5_proxy().await;
    
    let router = UpstreamRouter::from_config(&UpstreamConfig::Socks5 {
        addr: proxy_addr.to_string(),
        auth: None,
    });
    let mut stream = router.connector().connect(&ConnectTarget::Address(
        "127.0.0.1".into(), echo_addr.port()
    )).await.unwrap();
    
    use tokio::io::{AsyncWriteExt as _, AsyncReadExt as _};
    stream.write_all(b"Router via SOCKS5!").await.unwrap();
    
    let mut buf = vec![0u8; 4096];
    let n = stream.read(&mut buf).await.unwrap();
    assert_eq!(&buf[..n], b"Router via SOCKS5!");
}