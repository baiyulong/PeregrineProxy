use std::net::SocketAddr;
use tokio::net::TcpListener;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

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