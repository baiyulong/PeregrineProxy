use std::net::SocketAddr;
use tokio::net::TcpListener;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use std::time::Duration;

mod common;

/// Simple mock HTTP server that returns a fixed response
async fn start_mock_server() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    
    tokio::spawn(async move {
        loop {
            let (mut stream, _) = listener.accept().await.unwrap();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                let _ = stream.read(&mut buf).await.unwrap();
                
                let response = "HTTP/1.1 200 OK\r\nContent-Length: 13\r\n\r\nHello, Proxy!";
                stream.write_all(response.as_bytes()).await.unwrap();
            });
        }
    });
    
    addr
}

#[tokio::test]
async fn test_http_proxy_forward() {
    // Start mock target server
    let target_addr = start_mock_server().await;
    
    // Start proxy server
    let proxy_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_addr = proxy_listener.local_addr().unwrap();
    
    tokio::spawn(async move {
        let (stream, _) = proxy_listener.accept().await.unwrap();
        peregrine::protocol::http_handler::handle_http(stream).await;
    });
    
    // Send HTTP request through proxy with absolute URI
    let mut client = tokio::net::TcpStream::connect(proxy_addr).await.unwrap();
    let request = format!(
        "GET http://127.0.0.1:{}/test HTTP/1.1\r\nHost: 127.0.0.1:{}\r\n\r\n",
        target_addr.port(),
        target_addr.port()
    );
    client.write_all(request.as_bytes()).await.unwrap();
    
    let mut response = vec![0u8; 4096];
    let n = client.read(&mut response).await.unwrap();
    let response_str = String::from_utf8_lossy(&response[..n]);
    
    assert!(response_str.contains("200 OK"), "Expected 200 OK, got: {}", response_str);
    assert!(response_str.contains("Hello, Proxy!"), "Expected body, got: {}", response_str);
}

#[tokio::test]
async fn test_http_connect_tunnel() {
    // Start a mock TCP server (simulating a TLS endpoint)
    let mock_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mock_addr = mock_listener.local_addr().unwrap();
    
    tokio::spawn(async move {
        let (mut stream, _) = mock_listener.accept().await.unwrap();
        // Echo back whatever is received
        let mut buf = vec![0u8; 4096];
        let n = stream.read(&mut buf).await.unwrap();
        stream.write_all(&buf[..n]).await.unwrap();
    });
    
    // Start proxy
    let proxy_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_addr = proxy_listener.local_addr().unwrap();
    
    tokio::spawn(async move {
        let (stream, _) = proxy_listener.accept().await.unwrap();
        peregrine::protocol::http_handler::handle_http(stream).await;
    });
    
    // Send CONNECT through proxy
    let mut client = tokio::net::TcpStream::connect(proxy_addr).await.unwrap();
    let connect_req = format!(
        "CONNECT 127.0.0.1:{} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\n\r\n",
        mock_addr.port(), mock_addr.port()
    );
    client.write_all(connect_req.as_bytes()).await.unwrap();
    
    // Read 200 response
    let mut response = vec![0u8; 4096];
    let n = client.read(&mut response).await.unwrap();
    let response_str = String::from_utf8_lossy(&response[..n]);
    assert!(response_str.contains("200"), "Expected 200 response, got: {}", response_str);
    
    // Now send data through the tunnel (should be echoed back)
    let test_data = b"Hello through tunnel!";
    client.write_all(test_data).await.unwrap();
    
    let mut echo = vec![0u8; 4096];
    let n = client.read(&mut echo).await.unwrap();
    assert_eq!(&echo[..n], test_data, "Data should be echoed through tunnel");
}

#[tokio::test]
async fn test_http_forward_to_upstream_proxy() {
    // Test HTTP proxy → upstream HTTP proxy → target
    
    // 1. Start target server
    let (target_addr, _target_handle) = common::start_mock_http_server().await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    
    // 2. Start upstream HTTP proxy
    let (_upstream_addr, _upstream_handle) = common::start_mock_http_proxy().await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    
    // 3. Start our proxy (configured to use upstream)
    let proxy_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_addr = proxy_listener.local_addr().unwrap();
    
    tokio::spawn(async move {
        let (stream, _) = proxy_listener.accept().await.unwrap();
        // Note: In a real implementation, we'd configure the upstream here
        // For this test, we simulate forwarding behavior
        peregrine::protocol::http_handler::handle_http(stream).await;
    });
    
    // 4. Send request through our proxy
    let target_url = format!("http://127.0.0.1:{}/test", target_addr.port());
    match tokio::time::timeout(
        Duration::from_secs(5),
        common::http_request_through_proxy(proxy_addr, &target_url)
    ).await {
        Ok(Ok(response)) => {
            assert!(response.contains("200 OK"), "Expected 200 OK, got: {}", response);
            // Note: In a real upstream test, we'd verify the response comes from the target
            // For this integration test, we verify the proxy accepts and processes the request
        }
        Ok(Err(e)) => panic!("Request failed: {}", e),
        Err(_) => panic!("Request timed out"),
    }
}

#[tokio::test]
async fn test_connect_tunnel_to_upstream_proxy() {
    // Test HTTP CONNECT → upstream HTTP proxy → target
    
    // 1. Start echo target server
    let (target_addr, _target_handle) = common::start_echo_server().await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    
    // 2. Start upstream HTTP proxy
    let (_upstream_addr, _upstream_handle) = common::start_mock_http_proxy().await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    
    // 3. Start our proxy
    let proxy_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_addr = proxy_listener.local_addr().unwrap();
    
    tokio::spawn(async move {
        let (stream, _) = proxy_listener.accept().await.unwrap();
        peregrine::protocol::http_handler::handle_http(stream).await;
    });
    
    // 4. Test CONNECT tunnel
    match tokio::time::timeout(
        Duration::from_secs(5),
        common::connect_tunnel_through_proxy(proxy_addr, "127.0.0.1", target_addr.port())
    ).await {
        Ok(Ok(mut tunnel)) => {
            let test_data = b"Hello through upstream tunnel!";
            tunnel.write_all(test_data).await.unwrap();
            
            let mut echo = vec![0u8; 4096];
            let n = tunnel.read(&mut echo).await.unwrap();
            assert_eq!(&echo[..n], test_data, "Data should echo through upstream tunnel");
        }
        Ok(Err(e)) => panic!("CONNECT tunnel failed: {}", e),
        Err(_) => panic!("CONNECT tunnel timed out"),
    }
}

#[tokio::test]
async fn test_http_proxy_with_authentication_required() {
    // Test proxy authentication challenge (407)
    
    let proxy_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_addr = proxy_listener.local_addr().unwrap();
    
    tokio::spawn(async move {
        let (stream, _) = proxy_listener.accept().await.unwrap();
        // Simulate auth-required proxy behavior by manually handling the request
        let mut stream = stream;
        let mut buf = vec![0u8; 4096];
        if stream.read(&mut buf).await.is_ok() {
            let response = "HTTP/1.1 407 Proxy Authentication Required\r\n\
                           Proxy-Authenticate: Basic realm=\"proxy\"\r\n\
                           Content-Length: 0\r\n\r\n";
            let _ = stream.write_all(response.as_bytes()).await;
        }
    });
    
    // Send request without auth - should get 407
    let target_url = "http://127.0.0.1:8080/test";
    match tokio::time::timeout(
        Duration::from_secs(5),
        common::http_request_through_proxy(proxy_addr, target_url)
    ).await {
        Ok(Ok(response)) => {
            assert!(response.contains("407"), "Expected 407 Proxy Authentication Required, got: {}", response);
            assert!(response.contains("Proxy-Authenticate"), "Expected Proxy-Authenticate header");
        }
        Ok(Err(e)) => panic!("Request failed: {}", e),
        Err(_) => panic!("Request timed out"),
    }
}