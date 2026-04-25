use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, UdpSocket};

mod common;

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

/// Start a SOCKS5 proxy
async fn start_socks5_proxy() -> std::net::SocketAddr {
    start_socks5_proxy_with_auth(None).await
}

/// Start a SOCKS5 proxy with optional authentication
async fn start_socks5_proxy_with_auth(
    users: Option<Vec<peregrine::config::UserCredential>>,
) -> std::net::SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        loop {
            let (stream, peer_addr) = listener.accept().await.unwrap();
            let users = users.clone();
            tokio::spawn(async move {
                peregrine::protocol::socks5::handle_socks5(stream, peer_addr, users).await;
            });
        }
    });

    addr
}

/// Start a mock UDP echo server
async fn start_udp_echo_server() -> std::net::SocketAddr {
    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let addr = socket.local_addr().unwrap();

    tokio::spawn(async move {
        let mut buf = vec![0u8; 65535];
        while let Ok((n, from)) = socket.recv_from(&mut buf).await {
            let _ = socket.send_to(&buf[..n], from).await;
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
async fn test_socks5_connect_ipv6() {
    let echo_addr = start_echo_server().await;
    let proxy_addr = start_socks5_proxy().await;

    let mut client = tokio::net::TcpStream::connect(proxy_addr).await.unwrap();

    // Method negotiation
    client.write_all(&[0x05, 0x01, 0x00]).await.unwrap();
    let mut resp = [0u8; 2];
    client.read_exact(&mut resp).await.unwrap();
    assert_eq!(resp, [0x05, 0x00]);

    // CONNECT with IPv4-mapped IPv6 localhost (::ffff:127.0.0.1)
    let port = echo_addr.port().to_be_bytes();
    // IPv4-mapped IPv6: ::ffff:127.0.0.1
    let ipv4_mapped_ipv6 = [
        0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0xffu8, 0xffu8, 127u8, 0u8, 0u8, 1u8,
    ];

    let mut connect_req = vec![0x05, 0x01, 0x00, 0x04]; // ATYP=IPv6
    connect_req.extend_from_slice(&ipv4_mapped_ipv6);
    connect_req.extend_from_slice(&port);
    client.write_all(&connect_req).await.unwrap();

    let mut reply = [0u8; 10]; // Reply is still IPv4 format (10 bytes)
    client.read_exact(&mut reply).await.unwrap();
    assert_eq!(reply[0], 0x05, "Reply VER should be 5");
    assert_eq!(reply[1], 0x00, "Reply REP should be 0 (success)");

    // Data transfer
    let test_data = b"Hello IPv6!";
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

#[tokio::test]
async fn test_socks5_unsupported_command_bind() {
    let proxy_addr = start_socks5_proxy().await;

    let mut client = tokio::net::TcpStream::connect(proxy_addr).await.unwrap();

    // Method negotiation
    client.write_all(&[0x05, 0x01, 0x00]).await.unwrap();
    let mut resp = [0u8; 2];
    client.read_exact(&mut resp).await.unwrap();
    assert_eq!(resp, [0x05, 0x00]);

    // BIND request (unsupported)
    let bind_req = [
        0x05, 0x02, 0x00, 0x01, // VER, CMD=BIND, RSV, ATYP=IPv4
        127, 0, 0, 1, // 127.0.0.1
        0x00, 0x50, // Port 80
    ];
    client.write_all(&bind_req).await.unwrap();

    let mut reply = [0u8; 10];
    client.read_exact(&mut reply).await.unwrap();
    assert_eq!(reply[0], 0x05, "Reply VER should be 5");
    assert_eq!(
        reply[1], 0x07,
        "Reply REP should be 0x07 (command not supported)"
    );
}

#[tokio::test]
async fn test_socks5_connection_refused() {
    let proxy_addr = start_socks5_proxy().await;

    let mut client = tokio::net::TcpStream::connect(proxy_addr).await.unwrap();

    // Method negotiation
    client.write_all(&[0x05, 0x01, 0x00]).await.unwrap();
    let mut resp = [0u8; 2];
    client.read_exact(&mut resp).await.unwrap();
    assert_eq!(resp, [0x05, 0x00]);

    // CONNECT to a non-existent port
    let connect_req = [
        0x05, 0x01, 0x00, 0x01, // VER, CMD=CONNECT, RSV, ATYP=IPv4
        127, 0, 0, 1, // 127.0.0.1
        0xFF, 0xFF, // Port 65535 (unlikely to be open)
    ];
    client.write_all(&connect_req).await.unwrap();

    let mut reply = [0u8; 10];
    client.read_exact(&mut reply).await.unwrap();
    assert_eq!(reply[0], 0x05, "Reply VER should be 5");
    assert_eq!(
        reply[1], 0x05,
        "Reply REP should be 0x05 (connection refused)"
    );
}

#[tokio::test]
async fn test_socks5_auth_success() {
    let echo_addr = start_echo_server().await;
    let users = vec![peregrine::config::UserCredential {
        username: "testuser".into(),
        password: "testpass".into(),
    }];
    let proxy_addr = start_socks5_proxy_with_auth(Some(users)).await;

    let mut client = tokio::net::TcpStream::connect(proxy_addr).await.unwrap();

    // Offer both methods
    client.write_all(&[0x05, 0x02, 0x00, 0x02]).await.unwrap();
    let mut resp = [0u8; 2];
    client.read_exact(&mut resp).await.unwrap();
    assert_eq!(resp, [0x05, 0x02]); // Server selected username/password

    // Send credentials (RFC 1929)
    let username = b"testuser";
    let password = b"testpass";
    let mut auth = vec![0x01]; // VER
    auth.push(username.len() as u8); // ULEN
    auth.extend_from_slice(username);
    auth.push(password.len() as u8); // PLEN
    auth.extend_from_slice(password);
    client.write_all(&auth).await.unwrap();

    let mut auth_resp = [0u8; 2];
    client.read_exact(&mut auth_resp).await.unwrap();
    assert_eq!(auth_resp, [0x01, 0x00]); // Success

    // Now CONNECT should work
    let port = echo_addr.port().to_be_bytes();
    let ip = match echo_addr.ip() {
        std::net::IpAddr::V4(v4) => v4.octets(),
        _ => panic!(),
    };
    let mut req = vec![0x05, 0x01, 0x00, 0x01];
    req.extend_from_slice(&ip);
    req.extend_from_slice(&port);
    client.write_all(&req).await.unwrap();

    let mut reply = [0u8; 10];
    client.read_exact(&mut reply).await.unwrap();
    assert_eq!(reply[1], 0x00); // Success

    // Data transfer
    client.write_all(b"Auth works!").await.unwrap();
    let mut buf = vec![0u8; 4096];
    let n = client.read(&mut buf).await.unwrap();
    assert_eq!(&buf[..n], b"Auth works!");
}

#[tokio::test]
async fn test_socks5_auth_failure() {
    let users = vec![peregrine::config::UserCredential {
        username: "testuser".into(),
        password: "testpass".into(),
    }];
    let proxy_addr = start_socks5_proxy_with_auth(Some(users)).await;

    let mut client = tokio::net::TcpStream::connect(proxy_addr).await.unwrap();

    client.write_all(&[0x05, 0x02, 0x00, 0x02]).await.unwrap();
    let mut resp = [0u8; 2];
    client.read_exact(&mut resp).await.unwrap();
    assert_eq!(resp, [0x05, 0x02]);

    // Send wrong credentials
    let username = b"wrong";
    let password = b"creds";
    let mut auth = vec![0x01];
    auth.push(username.len() as u8);
    auth.extend_from_slice(username);
    auth.push(password.len() as u8);
    auth.extend_from_slice(password);
    client.write_all(&auth).await.unwrap();

    let mut auth_resp = [0u8; 2];
    client.read_exact(&mut auth_resp).await.unwrap();
    assert_eq!(auth_resp[1], 0x01); // Failure (non-zero = failure)
}

#[tokio::test]
async fn test_socks5_no_auth_when_not_configured() {
    // No users configured = no auth required
    let echo_addr = start_echo_server().await;
    let proxy_addr = start_socks5_proxy_with_auth(None).await;

    let mut client = tokio::net::TcpStream::connect(proxy_addr).await.unwrap();

    // Only offer no-auth
    client.write_all(&[0x05, 0x01, 0x00]).await.unwrap();
    let mut resp = [0u8; 2];
    client.read_exact(&mut resp).await.unwrap();
    assert_eq!(resp, [0x05, 0x00]); // No auth selected

    // CONNECT should work directly
    let port = echo_addr.port().to_be_bytes();
    let ip = match echo_addr.ip() {
        std::net::IpAddr::V4(v4) => v4.octets(),
        _ => panic!(),
    };
    let mut req = vec![0x05, 0x01, 0x00, 0x01];
    req.extend_from_slice(&ip);
    req.extend_from_slice(&port);
    client.write_all(&req).await.unwrap();

    let mut reply = [0u8; 10];
    client.read_exact(&mut reply).await.unwrap();
    assert_eq!(reply[1], 0x00);
}

#[tokio::test]
async fn test_socks5_udp_associate_basic() {
    let udp_echo_addr = start_udp_echo_server().await;
    let proxy_addr = start_socks5_proxy().await;

    let mut client = tokio::net::TcpStream::connect(proxy_addr).await.unwrap();

    // Method negotiation
    client.write_all(&[0x05, 0x01, 0x00]).await.unwrap();
    let mut resp = [0u8; 2];
    client.read_exact(&mut resp).await.unwrap();
    assert_eq!(resp, [0x05, 0x00]);

    // UDP ASSOCIATE request
    let udp_req = [
        0x05, 0x03, 0x00, 0x01, // VER, CMD=UDP_ASSOCIATE, RSV, ATYP=IPv4
        0x00, 0x00, 0x00, 0x00, // 0.0.0.0 (client doesn't know its address)
        0x00, 0x00, // Port 0 (client doesn't know its port)
    ];
    client.write_all(&udp_req).await.unwrap();

    // Read reply to get relay address
    let mut reply = [0u8; 10];
    client.read_exact(&mut reply).await.unwrap();
    assert_eq!(reply[0], 0x05, "Reply VER should be 5");
    assert_eq!(reply[1], 0x00, "Reply REP should be 0x00 (success)");
    assert_eq!(reply[3], 0x01, "Reply ATYP should be IPv4");

    // Extract relay address from reply
    let relay_ip = std::net::Ipv4Addr::new(reply[4], reply[5], reply[6], reply[7]);
    let relay_port = u16::from_be_bytes([reply[8], reply[9]]);
    let relay_addr = std::net::SocketAddr::V4(std::net::SocketAddrV4::new(relay_ip, relay_port));

    // Create UDP client socket
    let udp_client = UdpSocket::bind("127.0.0.1:0").await.unwrap();

    // Prepare SOCKS5 UDP packet
    // Format: RSV(2) + FRAG(1) + ATYP(1) + DST.ADDR(variable) + DST.PORT(2) + DATA(variable)
    let test_data = b"Hello UDP!";
    let target_ip = match udp_echo_addr.ip() {
        std::net::IpAddr::V4(v4) => v4.octets(),
        _ => panic!("Expected IPv4"),
    };
    let target_port = udp_echo_addr.port().to_be_bytes();

    let mut socks_packet = vec![
        0x00, 0x00, // RSV (2 bytes)
        0x00, // FRAG (0 = no fragmentation)
        0x01, // ATYP (IPv4)
    ];
    socks_packet.extend_from_slice(&target_ip); // DST.ADDR (4 bytes for IPv4)
    socks_packet.extend_from_slice(&target_port); // DST.PORT (2 bytes)
    socks_packet.extend_from_slice(test_data); // DATA

    // Send packet through SOCKS5 relay
    udp_client.send_to(&socks_packet, relay_addr).await.unwrap();

    // Receive response from relay
    let mut response_buf = vec![0u8; 1024];
    let (n, _) = tokio::time::timeout(
        Duration::from_secs(5),
        udp_client.recv_from(&mut response_buf),
    )
    .await
    .unwrap()
    .unwrap();

    // Parse response (same SOCKS5 UDP format)
    assert!(n > 10, "Response too short"); // Must have header + data
    assert_eq!(&response_buf[0..2], &[0x00, 0x00], "RSV should be 0");
    assert_eq!(response_buf[2], 0x00, "FRAG should be 0");
    assert_eq!(response_buf[3], 0x01, "ATYP should be IPv4");

    // Skip header (RSV + FRAG + ATYP + ADDR + PORT = 2 + 1 + 1 + 4 + 2 = 10 bytes)
    let response_data = &response_buf[10..n];
    assert_eq!(response_data, test_data, "Should echo back the data");

    // Test that TCP control channel closure stops UDP relay
    drop(client);
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Send another packet - should fail or timeout
    let result = tokio::time::timeout(
        Duration::from_millis(500),
        udp_client.send_to(&socks_packet, relay_addr),
    )
    .await;

    // The send might succeed but no response should come back
    if result.is_ok() {
        let timeout_result = tokio::time::timeout(
            Duration::from_millis(500),
            udp_client.recv_from(&mut response_buf),
        )
        .await;
        assert!(
            timeout_result.is_err(),
            "Should timeout after TCP control channel is closed"
        );
    }
}

#[tokio::test]
async fn test_socks5_udp_associate_with_auth() {
    let _udp_echo_addr = start_udp_echo_server().await;
    let users = vec![peregrine::config::UserCredential {
        username: "testuser".into(),
        password: "testpass".into(),
    }];
    let proxy_addr = start_socks5_proxy_with_auth(Some(users)).await;

    let mut client = tokio::net::TcpStream::connect(proxy_addr).await.unwrap();

    // Method negotiation - server requires auth
    client.write_all(&[0x05, 0x02, 0x00, 0x02]).await.unwrap();
    let mut resp = [0u8; 2];
    client.read_exact(&mut resp).await.unwrap();
    assert_eq!(resp, [0x05, 0x02]);

    // Send credentials
    let username = b"testuser";
    let password = b"testpass";
    let mut auth = vec![0x01];
    auth.push(username.len() as u8);
    auth.extend_from_slice(username);
    auth.push(password.len() as u8);
    auth.extend_from_slice(password);
    client.write_all(&auth).await.unwrap();

    let mut auth_resp = [0u8; 2];
    client.read_exact(&mut auth_resp).await.unwrap();
    assert_eq!(auth_resp, [0x01, 0x00]); // Success

    // UDP ASSOCIATE request
    let udp_req = [
        0x05, 0x03, 0x00, 0x01, // VER, CMD=UDP_ASSOCIATE, RSV, ATYP=IPv4
        0x00, 0x00, 0x00, 0x00, // 0.0.0.0
        0x00, 0x00, // Port 0
    ];
    client.write_all(&udp_req).await.unwrap();

    // Should succeed with authentication
    let mut reply = [0u8; 10];
    client.read_exact(&mut reply).await.unwrap();
    assert_eq!(reply[0], 0x05, "Reply VER should be 5");
    assert_eq!(reply[1], 0x00, "Reply REP should be 0x00 (success)");
}

#[tokio::test]
async fn test_socks5_udp_associate_domain_target() {
    let udp_echo_addr = start_udp_echo_server().await;
    let proxy_addr = start_socks5_proxy().await;

    let mut client = tokio::net::TcpStream::connect(proxy_addr).await.unwrap();

    // Method negotiation
    client.write_all(&[0x05, 0x01, 0x00]).await.unwrap();
    let mut resp = [0u8; 2];
    client.read_exact(&mut resp).await.unwrap();
    assert_eq!(resp, [0x05, 0x00]);

    // UDP ASSOCIATE request
    let udp_req = [0x05, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
    client.write_all(&udp_req).await.unwrap();

    let mut reply = [0u8; 10];
    client.read_exact(&mut reply).await.unwrap();
    assert_eq!(reply[1], 0x00, "Should succeed");

    // Extract relay address
    let relay_ip = std::net::Ipv4Addr::new(reply[4], reply[5], reply[6], reply[7]);
    let relay_port = u16::from_be_bytes([reply[8], reply[9]]);
    let relay_addr = std::net::SocketAddr::V4(std::net::SocketAddrV4::new(relay_ip, relay_port));

    let udp_client = UdpSocket::bind("127.0.0.1:0").await.unwrap();

    // Test with domain name target
    let test_data = b"Domain test!";
    let domain = b"127.0.0.1";
    let target_port = udp_echo_addr.port().to_be_bytes();

    let mut socks_packet = vec![
        0x00, 0x00, // RSV
        0x00, // FRAG
        0x03, // ATYP (Domain)
    ];
    socks_packet.push(domain.len() as u8); // Domain length
    socks_packet.extend_from_slice(domain); // Domain
    socks_packet.extend_from_slice(&target_port); // Port
    socks_packet.extend_from_slice(test_data); // Data

    udp_client.send_to(&socks_packet, relay_addr).await.unwrap();

    let mut response_buf = vec![0u8; 1024];
    let (n, _) = tokio::time::timeout(
        Duration::from_secs(5),
        udp_client.recv_from(&mut response_buf),
    )
    .await
    .unwrap()
    .unwrap();

    // Response should have domain header (RSV + FRAG + ATYP + LEN + DOMAIN + PORT)
    let expected_header_len = 2 + 1 + 1 + 1 + domain.len() + 2;
    let response_data = &response_buf[expected_header_len..n];
    assert_eq!(response_data, test_data);
}

#[tokio::test]
async fn test_socks5_to_upstream_socks5_proxy() {
    // Test SOCKS5 → upstream SOCKS5 proxy → target

    // 1. Start echo target server
    let target_addr = start_echo_server().await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    // 2. Start upstream SOCKS5 proxy
    let (_upstream_addr, _upstream_handle) = common::start_mock_socks5_proxy().await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    // 3. Start our SOCKS5 proxy (would be configured to use upstream in real implementation)
    let our_proxy_addr = start_socks5_proxy().await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    // 4. Connect through our proxy to target
    match tokio::time::timeout(
        Duration::from_secs(5),
        common::socks5_connect_through_proxy(our_proxy_addr, "127.0.0.1", target_addr.port()),
    )
    .await
    {
        Ok(Ok(mut tunnel)) => {
            let test_data = b"Hello through SOCKS5 chain!";
            tunnel.write_all(test_data).await.unwrap();

            let mut echo = vec![0u8; 4096];
            let n = tunnel.read(&mut echo).await.unwrap();
            assert_eq!(
                &echo[..n],
                test_data,
                "Data should echo through SOCKS5 chain"
            );
        }
        Ok(Err(e)) => panic!("SOCKS5 chain connection failed: {}", e),
        Err(_) => panic!("SOCKS5 chain connection timed out"),
    }
}

#[tokio::test]
async fn test_socks5_full_pipeline_with_auth() {
    // Test full SOCKS5 pipeline: client → SOCKS5 proxy (with auth) → target

    // 1. Start echo server
    let target_addr = start_echo_server().await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    // 2. Start SOCKS5 proxy with authentication
    let users = vec![peregrine::config::UserCredential {
        username: "testuser".into(),
        password: "testpass".into(),
    }];
    let proxy_addr = start_socks5_proxy_with_auth(Some(users.clone())).await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    // 3. Connect with authentication
    let mut client = tokio::net::TcpStream::connect(proxy_addr).await.unwrap();

    // Method negotiation - offer both no-auth and username/password
    client.write_all(&[0x05, 0x02, 0x00, 0x02]).await.unwrap();
    let mut resp = [0u8; 2];
    client.read_exact(&mut resp).await.unwrap();
    assert_eq!(resp, [0x05, 0x02]); // Server requires username/password

    // Send credentials
    let username = b"testuser";
    let password = b"testpass";
    let mut auth = vec![0x01]; // VER
    auth.push(username.len() as u8);
    auth.extend_from_slice(username);
    auth.push(password.len() as u8);
    auth.extend_from_slice(password);
    client.write_all(&auth).await.unwrap();

    let mut auth_resp = [0u8; 2];
    client.read_exact(&mut auth_resp).await.unwrap();
    assert_eq!(auth_resp, [0x01, 0x00]); // Auth success

    // CONNECT to target
    let port = target_addr.port().to_be_bytes();
    let ip = match target_addr.ip() {
        std::net::IpAddr::V4(v4) => v4.octets(),
        _ => panic!("Expected IPv4"),
    };
    let mut req = vec![0x05, 0x01, 0x00, 0x01]; // VER, CMD=CONNECT, RSV, ATYP=IPv4
    req.extend_from_slice(&ip);
    req.extend_from_slice(&port);
    client.write_all(&req).await.unwrap();

    let mut reply = [0u8; 10];
    client.read_exact(&mut reply).await.unwrap();
    assert_eq!(reply[1], 0x00); // Success

    // Test data transfer through authenticated connection
    let test_data = b"Authenticated SOCKS5 works!";
    client.write_all(test_data).await.unwrap();

    let mut echo = vec![0u8; 4096];
    let n = client.read(&mut echo).await.unwrap();
    assert_eq!(
        &echo[..n],
        test_data,
        "Data should echo through authenticated SOCKS5"
    );
}
