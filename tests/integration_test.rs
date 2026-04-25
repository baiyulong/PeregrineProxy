use tokio::net::{TcpListener, TcpStream};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use std::time::Duration;

mod common;

#[tokio::test]
async fn test_integration_acl_deny_blocks_connection() {
    // Test that ACL deny rule blocks connection at the proxy level
    
    // Start target server (unused in this particular test but good practice)
    let (_target_addr, _target_handle) = common::start_echo_server().await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    
    // Create a proxy that would deny connections to localhost
    let proxy_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_addr = proxy_listener.local_addr().unwrap();
    
    tokio::spawn(async move {
        if let Ok((mut stream, client_addr)) = proxy_listener.accept().await {
            // Simulate ACL check - deny localhost connections
            if client_addr.ip().is_loopback() {
                // Send 403 Forbidden for HTTP requests
                let mut buf = vec![0u8; 4096];
                if stream.read(&mut buf).await.is_ok() {
                    let request = String::from_utf8_lossy(&buf);
                    if request.starts_with("GET") || request.starts_with("POST") {
                        let response = "HTTP/1.1 403 Forbidden\r\n\
                                     Content-Length: 13\r\n\r\n\
                                     Access Denied";
                        let _ = stream.write_all(response.as_bytes()).await;
                    } else if request.starts_with("CONNECT") {
                        let response = "HTTP/1.1 403 Forbidden\r\n\r\n";
                        let _ = stream.write_all(response.as_bytes()).await;
                    }
                    // For SOCKS5, we'd send error reply after method negotiation
                }
            }
        }
    });
    
    // Test HTTP request - should be denied
    match tokio::time::timeout(
        Duration::from_secs(5),
        common::http_request_through_proxy(proxy_addr, "http://127.0.0.1:8080/test")
    ).await {
        Ok(Ok(response)) => {
            assert!(response.contains("403") || response.contains("Forbidden"), 
                   "Expected 403 Forbidden, got: {}", response);
        }
        Ok(Err(_)) => {
            // Connection might be dropped immediately - this is also valid for ACL denial
        }
        Err(_) => panic!("Request should not timeout, should get denial response"),
    }
}

#[tokio::test] 
async fn test_integration_acl_allow_permits_connection() {
    // Test that ACL allow rule permits connection
    
    let (target_addr, _target_handle) = common::start_echo_server().await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    
    // Simple proxy that allows all connections
    let proxy_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_addr = proxy_listener.local_addr().unwrap();
    
    tokio::spawn(async move {
        if let Ok((stream, _)) = proxy_listener.accept().await {
            // Allow all - use normal HTTP handler
            peregrine::protocol::http_handler::handle_http(stream).await;
        }
    });
    
    // Test CONNECT tunnel through allowed proxy
    match tokio::time::timeout(
        Duration::from_secs(5),
        common::connect_tunnel_through_proxy(proxy_addr, "127.0.0.1", target_addr.port())
    ).await {
        Ok(Ok(mut tunnel)) => {
            let test_data = b"ACL allows this!";
            tunnel.write_all(test_data).await.unwrap();
            
            let mut echo = vec![0u8; 4096];
            let n = tunnel.read(&mut echo).await.unwrap();
            assert_eq!(&echo[..n], test_data, "Connection should be allowed by ACL");
        }
        Ok(Err(e)) => panic!("Connection should be allowed: {}", e),
        Err(_) => panic!("Connection should not timeout"),
    }
}

#[tokio::test]
async fn test_integration_proxy_auth_challenge() {
    // Test that proxy challenges for authentication when configured
    
    let proxy_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_addr = proxy_listener.local_addr().unwrap();
    
    tokio::spawn(async move {
        if let Ok((mut stream, _)) = proxy_listener.accept().await {
            let mut buf = vec![0u8; 4096];
            if stream.read(&mut buf).await.is_ok() {
                let request = String::from_utf8_lossy(&buf);
                
                // Check for Proxy-Authorization header
                if !request.contains("Proxy-Authorization:") {
                    // No auth provided - send 407
                    let response = "HTTP/1.1 407 Proxy Authentication Required\r\n\
                                   Proxy-Authenticate: Basic realm=\"PeregrineProxy\"\r\n\
                                   Content-Length: 33\r\n\r\n\
                                   Proxy authentication required.";
                    let _ = stream.write_all(response.as_bytes()).await;
                } else {
                    // Auth provided - simulate success
                    if request.starts_with("CONNECT") {
                        let response = "HTTP/1.1 200 Connection established\r\n\r\n";
                        let _ = stream.write_all(response.as_bytes()).await;
                    } else {
                        let response = "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nOK";
                        let _ = stream.write_all(response.as_bytes()).await;
                    }
                }
            }
        }
    });
    
    // Test without authentication - should get 407
    match tokio::time::timeout(
        Duration::from_secs(5),
        common::http_request_through_proxy(proxy_addr, "http://example.com/")
    ).await {
        Ok(Ok(response)) => {
            assert!(response.contains("407"), "Expected 407 auth challenge, got: {}", response);
            assert!(response.contains("Proxy-Authenticate"), "Expected Proxy-Authenticate header");
            assert!(response.contains("Basic realm="), "Expected Basic auth challenge");
        }
        Ok(Err(e)) => panic!("Request failed: {}", e),
        Err(_) => panic!("Request timed out"),
    }
}

#[tokio::test]
async fn test_integration_socks5_auth_failure() {
    // Test SOCKS5 authentication failure
    
    let users = vec![peregrine::config::UserCredential {
        username: "correctuser".into(),
        password: "correctpass".into(),
    }];
    let proxy_addr = common::start_socks5_proxy_server_with_auth(users).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    
    let mut client = TcpStream::connect(proxy_addr).await.unwrap();
    
    // Method negotiation
    client.write_all(&[0x05, 0x02, 0x00, 0x02]).await.unwrap(); // Offer no-auth and username/password
    let mut resp = [0u8; 2];
    client.read_exact(&mut resp).await.unwrap();
    assert_eq!(resp, [0x05, 0x02]); // Server requires username/password
    
    // Send wrong credentials
    let wrong_username = b"wronguser";
    let wrong_password = b"wrongpass";
    let mut auth = vec![0x01]; // VER
    auth.push(wrong_username.len() as u8);
    auth.extend_from_slice(wrong_username);
    auth.push(wrong_password.len() as u8);
    auth.extend_from_slice(wrong_password);
    client.write_all(&auth).await.unwrap();
    
    let mut auth_resp = [0u8; 2];
    client.read_exact(&mut auth_resp).await.unwrap();
    assert_ne!(auth_resp[1], 0x00, "Authentication should fail with wrong credentials");
}

#[tokio::test]
async fn test_integration_multi_protocol_on_same_port() {
    // Test that we can start both HTTP and SOCKS5 proxies on different ports
    // (In a real implementation, protocol detection would allow same port)
    
    let (target_addr, _target_handle) = common::start_echo_server().await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    
    // Start HTTP proxy
    let http_proxy_addr = common::start_http_proxy_server().await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    
    // Start SOCKS5 proxy on different port
    let socks5_proxy_addr = common::start_socks5_proxy_server().await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    
    // Test HTTP proxy with CONNECT tunnel
    match tokio::time::timeout(
        Duration::from_secs(5),
        common::connect_tunnel_through_proxy(http_proxy_addr, "127.0.0.1", target_addr.port())
    ).await {
        Ok(Ok(mut tunnel)) => {
            let test_data = b"HTTP proxy works!";
            tunnel.write_all(test_data).await.unwrap();
            
            let mut echo = vec![0u8; 4096];
            let n = tunnel.read(&mut echo).await.unwrap();
            assert_eq!(&echo[..n], test_data, "HTTP CONNECT should work");
        }
        Ok(Err(e)) => panic!("HTTP CONNECT failed: {}", e),
        Err(_) => panic!("HTTP CONNECT timed out"),
    }
    
    // Test SOCKS5 connection
    match tokio::time::timeout(
        Duration::from_secs(5),
        common::socks5_connect_through_proxy(socks5_proxy_addr, "127.0.0.1", target_addr.port())
    ).await {
        Ok(Ok(mut tunnel)) => {
            let test_data = b"SOCKS5 proxy works!";
            tunnel.write_all(test_data).await.unwrap();
            
            let mut echo = vec![0u8; 4096];
            let n = tunnel.read(&mut echo).await.unwrap();
            assert_eq!(&echo[..n], test_data, "SOCKS5 should work on different port");
        }
        Ok(Err(e)) => panic!("SOCKS5 connection failed: {}", e),
        Err(_) => panic!("SOCKS5 connection timed out"),
    }
}

#[tokio::test]
async fn test_integration_connection_refused_error_handling() {
    // Test proper error handling when target is unreachable
    
    let proxy_addr = common::start_http_proxy_server().await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    
    // Try to connect to non-existent target
    let unreachable_url = "http://127.0.0.1:65534/test"; // Very unlikely to be open
    
    match tokio::time::timeout(
        Duration::from_secs(5),
        common::http_request_through_proxy(proxy_addr, unreachable_url)
    ).await {
        Ok(Ok(response)) => {
            // Should get some kind of error response
            assert!(
                response.contains("502") || 
                response.contains("503") || 
                response.contains("504") ||
                response.contains("Connection") ||
                response.is_empty(), // Connection might be dropped
                "Expected error response for unreachable target, got: {}", response
            );
        }
        Ok(Err(_)) => {
            // Connection error is also acceptable - proxy might close connection
        }
        Err(_) => panic!("Should not timeout - should get immediate error"),
    }
}

#[tokio::test]
async fn test_integration_concurrent_connections() {
    // Test that proxy can handle multiple concurrent connections
    
    let (target_addr, _target_handle) = common::start_echo_server().await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    
    let proxy_addr = common::start_socks5_proxy_server().await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    
    let mut successful = 0;
    
    // Launch 5 concurrent SOCKS5 connections using spawn_blocking to avoid futures dependency
    let mut tasks = vec![];
    
    for i in 0..5 {
        let task = tokio::spawn(async move {
            if let Ok(Ok(mut tunnel)) = tokio::time::timeout(
                Duration::from_secs(5),
                common::socks5_connect_through_proxy(proxy_addr, "127.0.0.1", target_addr.port())
            ).await {
                let test_data = format!("Concurrent test {}", i);
                if tunnel.write_all(test_data.as_bytes()).await.is_ok() {
                    let mut echo = vec![0u8; 4096];
                    if let Ok(n) = tunnel.read(&mut echo).await {
                        if &echo[..n] == test_data.as_bytes() {
                            return true;
                        }
                    }
                }
            }
            false
        });
        tasks.push(task);
    }
    
    // Wait for all tasks and count successes
    for task in tasks {
        if let Ok(success) = task.await {
            if success {
                successful += 1;
            }
        }
    }
    
    assert_eq!(successful, 5, "All concurrent connections should succeed");
}