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