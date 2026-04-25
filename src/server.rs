use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::{Semaphore, Notify};
use crate::config::AppConfig;
use crate::protocol::detect::{detect_protocol, DetectedProtocol};
use crate::protocol::http_handler;
use crate::protocol::socks5;

pub async fn run(config: AppConfig) -> anyhow::Result<()> {
    let max_conn = config.server.max_connections.unwrap_or(10000);
    let semaphore = Arc::new(Semaphore::new(max_conn));
    let shutdown = Arc::new(Notify::new());
    
    let mut listener_tasks = Vec::new();
    
    for listen_cfg in &config.server.listen {
        let listener = TcpListener::bind(&listen_cfg.addr).await?;
        let protocol = listen_cfg.protocol.clone();
        let sem = semaphore.clone();
        let shutdown_signal = shutdown.clone();
        
        tracing::info!("Listening on {} ({})", listen_cfg.addr, protocol_name(&protocol));
        
        let task = tokio::spawn(async move {
            accept_loop(listener, sem, shutdown_signal).await;
        });
        listener_tasks.push(task);
    }
    
    // Wait for shutdown signal
    tokio::signal::ctrl_c().await?;
    tracing::info!("Shutdown signal received, stopping accept loops...");
    shutdown.notify_waiters();
    
    // Wait for accept loops to stop
    for task in listener_tasks {
        let _ = task.await;
    }
    
    // Wait for in-flight connections to drain
    let active_connections = max_conn - semaphore.available_permits();
    if active_connections > 0 {
        tracing::info!("Waiting for {} active connections to drain...", active_connections);
        
        let drain_result = tokio::time::timeout(
            Duration::from_secs(30),
            wait_for_connections(semaphore.clone(), max_conn),
        ).await;
        
        match drain_result {
            Ok(_) => tracing::info!("All connections drained gracefully"),
            Err(_) => {
                let remaining = max_conn - semaphore.available_permits();
                tracing::warn!("Shutdown timeout, forcing close with {} remaining connections", remaining);
            }
        }
    } else {
        tracing::info!("No active connections to drain");
    }
    
    Ok(())
}

async fn accept_loop(listener: TcpListener, semaphore: Arc<Semaphore>, shutdown: Arc<Notify>) {
    loop {
        tokio::select! {
            result = listener.accept() => {
                match result {
                    Ok((stream, addr)) => {
                        let permit = semaphore.clone().acquire_owned().await;
                        match permit {
                            Ok(permit) => {
                                tokio::spawn(async move {
                                    tracing::info!("New connection from {}", addr);
                                    // TODO: protocol detection (Task 5)
                                    handle_connection(stream, addr).await;
                                    drop(permit);
                                });
                            }
                            Err(_) => {
                                tracing::error!("Semaphore closed");
                                break;
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!("Failed to accept connection: {}", e);
                    }
                }
            }
            _ = shutdown.notified() => {
                tracing::info!("Accept loop shutting down");
                break;
            }
        }
    }
}

async fn wait_for_connections(semaphore: Arc<Semaphore>, max_conn: usize) {
    // When we can acquire all permits, all connections are done
    let _ = semaphore.acquire_many(max_conn as u32).await;
}

async fn handle_connection(stream: tokio::net::TcpStream, addr: std::net::SocketAddr) {
    let mut buf = [0u8; 8];
    match stream.peek(&mut buf).await {
        Ok(0) => {
            tracing::debug!("Connection from {} closed before sending data", addr);
            return;
        }
        Ok(_) => {}
        Err(e) => {
            tracing::error!("Failed to peek from {}: {}", addr, e);
            return;
        }
    }
    
    match detect_protocol(&buf) {
        DetectedProtocol::Http => {
            tracing::debug!("HTTP protocol detected from {}", addr);
            http_handler::handle_http(stream).await;
        }
        DetectedProtocol::Socks5 => {
            tracing::debug!("SOCKS5 protocol detected from {}", addr);
            socks5::handle_socks5(stream, addr, None).await;
        }
        DetectedProtocol::Unknown => {
            tracing::warn!("Unknown protocol from {}, closing connection", addr);
        }
    }
}

fn protocol_name(protocol: &crate::config::Protocol) -> &'static str {
    match protocol {
        crate::config::Protocol::Http => "HTTP",
        crate::config::Protocol::Socks5 => "SOCKS5",
    }
}