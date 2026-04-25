//! Integration tests for metrics HTTP server
use peregrine::metrics::{ProxyMetrics, ProtocolLabel, ActionLabel, RequestLabels};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::test]
async fn test_metrics_http_server_endpoints() {
    // Find a free port
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener); // Release the listener
    
    let metrics = Arc::new(ProxyMetrics::new());
    
    // Add some test data
    metrics.connections_active.inc_by(42);
    metrics.upstream_errors_total.inc_by(7);
    
    let labels = RequestLabels {
        protocol: ProtocolLabel::http,
        action: ActionLabel::forward,
    };
    metrics.requests_total.get_or_create(&labels).inc_by(123);
    metrics.request_duration_seconds.observe(0.5);
    
    // Start the metrics server
    let _server_handle = ProxyMetrics::start_server(Arc::clone(&metrics), addr.to_string()).await;
    
    // Give the server a moment to start
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    
    // Test /metrics endpoint
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream.write_all(b"GET /metrics HTTP/1.1\r\nHost: localhost\r\n\r\n").await.unwrap();
    
    let mut buffer = [0; 4096];
    let n = stream.read(&mut buffer).await.unwrap();
    let response = String::from_utf8_lossy(&buffer[..n]);
    
    // Debug: print the actual response to see what we get
    println!("Actual response:\n{}", response);
    
    assert!(response.starts_with("HTTP/1.1 200 OK"));
    assert!(response.contains("Content-Type: text/plain"));
    assert!(response.contains("peregrine_connections_active"));
    assert!(response.contains("peregrine_upstream_errors_total"));
    assert!(response.contains("peregrine_requests_total"));
    assert!(response.contains("peregrine_request_duration_seconds"));
    
    // Check that values are present somewhere in the response
    assert!(response.contains("42"));
    assert!(response.contains("7"));
    assert!(response.contains("123"));
    
    drop(stream);
    
    // Test /health endpoint
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream.write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\n\r\n").await.unwrap();
    
    let mut buffer = [0; 512];
    let n = stream.read(&mut buffer).await.unwrap();
    let response = String::from_utf8_lossy(&buffer[..n]);
    
    assert!(response.starts_with("HTTP/1.1 200 OK"));
    assert!(response.contains("Content-Type: text/plain"));
    assert!(response.ends_with("OK"));
    
    drop(stream);
    
    // Test 404 for unknown endpoint
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream.write_all(b"GET /unknown HTTP/1.1\r\nHost: localhost\r\n\r\n").await.unwrap();
    
    let mut buffer = [0; 512];
    let n = stream.read(&mut buffer).await.unwrap();
    let response = String::from_utf8_lossy(&buffer[..n]);
    
    assert!(response.starts_with("HTTP/1.1 404 Not Found"));
    assert!(response.contains("Not Found"));
}