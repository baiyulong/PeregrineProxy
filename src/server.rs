use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::Semaphore;
use crate::config::AppConfig;
use crate::protocol::detect::{detect_protocol, DetectedProtocol};

pub async fn run(config: AppConfig) -> anyhow::Result<()> {
    let max_conn = config.server.max_connections.unwrap_or(10000);
    let semaphore = Arc::new(Semaphore::new(max_conn));
    
    let mut listener_tasks = Vec::new();
    
    for listen_cfg in &config.server.listen {
        let listener = TcpListener::bind(&listen_cfg.addr).await?;
        let protocol = listen_cfg.protocol.clone();
        let sem = semaphore.clone();
        
        tracing::info!("Listening on {} ({})", listen_cfg.addr, protocol_name(&protocol));
        
        let task = tokio::spawn(async move {
            accept_loop(listener, sem).await;
        });
        listener_tasks.push(task);
    }
    
    // Wait for shutdown signal
    tokio::signal::ctrl_c().await?;
    tracing::info!("Shutdown signal received, stopping...");
    
    // Abort all listener tasks
    for task in &listener_tasks {
        task.abort();
    }
    
    Ok(())
}

async fn accept_loop(listener: TcpListener, semaphore: Arc<Semaphore>) {
    loop {
        match listener.accept().await {
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
            // TODO: handle_http(stream).await (Task 6)
        }
        DetectedProtocol::Socks5 => {
            tracing::debug!("SOCKS5 protocol detected from {}", addr);
            // TODO: handle_socks5(stream).await (Task 8)
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