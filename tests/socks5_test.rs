use tokio::net::TcpListener;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Start a mock TCP echo server
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

/// Start a SOCKS5 proxy
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
async fn test_socks5_connect_ipv4() {
    let echo_addr = start_echo_server().await;
    let proxy_addr = start_socks5_proxy().await;
    
    let mut client = tokio::net::TcpStream::connect(proxy_addr).await.unwrap();
    
    // Phase 1: Method negotiation (no auth)
    client.write_all(&[0x05, 0x01, 0x00]).await.unwrap(); // VER, NMETHODS=1, METHOD=0 (no auth)
    
    let mut resp = [0u8; 2];
    client.read_exact(&mut resp).await.unwrap();
    assert_eq!(resp, [0x05, 0x00], "Should accept no-auth method"); // VER=5, METHOD=0
    
    // Phase 2: CONNECT request with IPv4
    let ip = echo_addr.ip();
    let port = echo_addr.port();
    let ip_bytes = match ip {
        std::net::IpAddr::V4(v4) => v4.octets(),
        _ => panic!("Expected IPv4"),
    };
    let port_bytes = port.to_be_bytes();
    
    let mut connect_req = vec![0x05, 0x01, 0x00, 0x01]; // VER, CMD=CONNECT, RSV, ATYP=IPv4
    connect_req.extend_from_slice(&ip_bytes);
    connect_req.extend_from_slice(&port_bytes);
    client.write_all(&connect_req).await.unwrap();
    
    // Read reply
    let mut reply = [0u8; 10]; // IPv4 reply is 10 bytes
    client.read_exact(&mut reply).await.unwrap();
    assert_eq!(reply[0], 0x05, "Reply VER should be 5");
    assert_eq!(reply[1], 0x00, "Reply REP should be 0 (success)");
    
    // Phase 3: Data transfer through tunnel
    let test_data = b"Hello SOCKS5!";
    client.write_all(test_data).await.unwrap();
    
    let mut echo = vec![0u8; 4096];
    let n = client.read(&mut echo).await.unwrap();
    assert_eq!(&echo[..n], test_data);
}

#[tokio::test]
async fn test_socks5_connect_domain() {
    let echo_addr = start_echo_server().await;
    let proxy_addr = start_socks5_proxy().await;
    
    let mut client = tokio::net::TcpStream::connect(proxy_addr).await.unwrap();
    
    // Method negotiation
    client.write_all(&[0x05, 0x01, 0x00]).await.unwrap();
    let mut resp = [0u8; 2];
    client.read_exact(&mut resp).await.unwrap();
    assert_eq!(resp, [0x05, 0x00]);
    
    // CONNECT with domain name
    let domain = b"127.0.0.1";
    let port = echo_addr.port().to_be_bytes();
    
    let mut connect_req = vec![0x05, 0x01, 0x00, 0x03]; // ATYP=Domain
    connect_req.push(domain.len() as u8);
    connect_req.extend_from_slice(domain);
    connect_req.extend_from_slice(&port);
    client.write_all(&connect_req).await.unwrap();
    
    let mut reply = [0u8; 10];
    client.read_exact(&mut reply).await.unwrap();
    assert_eq!(reply[1], 0x00, "Should succeed");
    
    // Data transfer
    let test_data = b"Hello via domain!";
    client.write_all(test_data).await.unwrap();
    
    let mut echo = vec![0u8; 4096];
    let n = client.read(&mut echo).await.unwrap();
    assert_eq!(&echo[..n], test_data);
}

#[tokio::test]
async fn test_socks5_unsupported_method() {
    let proxy_addr = start_socks5_proxy().await;
    
    let mut client = tokio::net::TcpStream::connect(proxy_addr).await.unwrap();
    
    // Only offer auth method 0x02 (username/password), no 0x00
    client.write_all(&[0x05, 0x01, 0x02]).await.unwrap();
    
    let mut resp = [0u8; 2];
    client.read_exact(&mut resp).await.unwrap();
    assert_eq!(resp, [0x05, 0xFF], "Should reject - no acceptable methods");
}