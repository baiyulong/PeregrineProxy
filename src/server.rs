use crate::config::{AppConfig, TlsConfig};
use crate::protocol::detect::{detect_protocol, DetectedProtocol};
use crate::protocol::http_handler;
use crate::protocol::socks5;
use rustls::pki_types::CertificateDer;
use rustls::ServerConfig;
use std::io::BufReader;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{watch, Notify, Semaphore};
use tokio_rustls::{server::TlsStream, TlsAcceptor};

#[cfg(unix)]
use tokio::signal::unix::{signal, SignalKind};

/// A stream that can be either plain TCP or TLS-wrapped
pub enum ProxyStream {
    Plain(TcpStream),
    Tls(Box<TlsStream<TcpStream>>),
}

impl AsyncRead for ProxyStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        match &mut *self {
            ProxyStream::Plain(stream) => Pin::new(stream).poll_read(cx, buf),
            ProxyStream::Tls(stream) => Pin::new(stream).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for ProxyStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, std::io::Error>> {
        match &mut *self {
            ProxyStream::Plain(stream) => Pin::new(stream).poll_write(cx, buf),
            ProxyStream::Tls(stream) => Pin::new(stream).poll_write(cx, buf),
        }
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        match &mut *self {
            ProxyStream::Plain(stream) => Pin::new(stream).poll_flush(cx),
            ProxyStream::Tls(stream) => Pin::new(stream).poll_flush(cx),
        }
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        match &mut *self {
            ProxyStream::Plain(stream) => Pin::new(stream).poll_shutdown(cx),
            ProxyStream::Tls(stream) => Pin::new(stream).poll_shutdown(cx),
        }
    }
}

impl ProxyStream {
    /// Peek at the stream buffer to detect protocol
    pub async fn peek(&self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            ProxyStream::Plain(stream) => stream.peek(buf).await,
            ProxyStream::Tls(_) => {
                // For TLS streams, we can't peek since data is encrypted
                // We'll need a different approach for TLS protocol detection
                Err(std::io::Error::new(
                    std::io::ErrorKind::Unsupported,
                    "Peek not supported on TLS streams",
                ))
            }
        }
    }
}

fn load_tls_acceptor(config: &TlsConfig) -> anyhow::Result<TlsAcceptor> {
    let cert_file = std::fs::File::open(&config.cert)
        .map_err(|e| anyhow::anyhow!("Failed to open cert file {}: {}", config.cert, e))?;
    let key_file = std::fs::File::open(&config.key)
        .map_err(|e| anyhow::anyhow!("Failed to open key file {}: {}", config.key, e))?;

    let certs: Vec<CertificateDer> = rustls_pemfile::certs(&mut BufReader::new(cert_file))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| anyhow::anyhow!("Failed to parse certificates: {}", e))?;

    if certs.is_empty() {
        return Err(anyhow::anyhow!("No certificates found in {}", config.cert));
    }

    let key = rustls_pemfile::private_key(&mut BufReader::new(key_file))
        .map_err(|e| anyhow::anyhow!("Failed to parse private key: {}", e))?
        .ok_or_else(|| anyhow::anyhow!("No private key found in {}", config.key))?;

    let server_config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| anyhow::anyhow!("Failed to build TLS config: {}", e))?;

    Ok(TlsAcceptor::from(Arc::new(server_config)))
}

pub async fn run(config: AppConfig, config_path: &str) -> anyhow::Result<()> {
    // Create a watch channel for config updates
    let (config_tx, config_rx) = watch::channel(config.clone());

    // Store the current config
    let current_config = config_rx.borrow().clone();
    let max_conn = current_config.server.max_connections.unwrap_or(10000);
    let semaphore = Arc::new(Semaphore::new(max_conn));
    let shutdown = Arc::new(Notify::new());

    // Spawn SIGHUP listener for config reloading (Unix only)
    #[cfg(unix)]
    {
        let config_path_owned = config_path.to_string();
        let config_tx_clone = config_tx.clone();
        tokio::spawn(async move {
            let mut sighup = match signal(SignalKind::hangup()) {
                Ok(s) => s,
                Err(e) => {
                    tracing::error!("Failed to create SIGHUP signal handler: {}", e);
                    return;
                }
            };

            loop {
                sighup.recv().await;
                tracing::info!(
                    "SIGHUP received, reloading config from {}",
                    config_path_owned
                );

                match AppConfig::reload(&config_path_owned) {
                    Ok(new_config) => {
                        if let Err(e) = config_tx_clone.send(new_config) {
                            tracing::error!("Failed to update config: {}", e);
                        } else {
                            tracing::info!("Config reloaded successfully");
                        }
                    }
                    Err(e) => {
                        tracing::error!("Failed to reload config: {}", e);
                    }
                }
            }
        });
    }

    #[cfg(not(unix))]
    {
        eprintln!("Note: config hot-reload via SIGHUP is only supported on Unix systems");
    }

    let mut listener_tasks = Vec::new();

    for listen_cfg in &current_config.server.listen {
        let listener = TcpListener::bind(&listen_cfg.addr).await?;
        let protocol = listen_cfg.protocol.clone();
        let tls_acceptor = match &listen_cfg.tls {
            Some(tls_config) => Some(load_tls_acceptor(tls_config)?),
            None => None,
        };
        let sem = semaphore.clone();
        let shutdown_signal = shutdown.clone();

        let protocol_label = if tls_acceptor.is_some() {
            format!("{} with TLS", protocol_name(&protocol))
        } else {
            protocol_name(&protocol).to_string()
        };
        eprintln!("Listening on {} ({})", listen_cfg.addr, protocol_label);
        tracing::info!("Listening on {} ({})", listen_cfg.addr, protocol_label);

        let task = tokio::spawn(async move {
            accept_loop(listener, tls_acceptor, sem, shutdown_signal).await;
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
        tracing::info!(
            "Waiting for {} active connections to drain...",
            active_connections
        );

        let drain_result = tokio::time::timeout(
            Duration::from_secs(30),
            wait_for_connections(semaphore.clone(), max_conn),
        )
        .await;

        match drain_result {
            Ok(_) => tracing::info!("All connections drained gracefully"),
            Err(_) => {
                let remaining = max_conn - semaphore.available_permits();
                tracing::warn!(
                    "Shutdown timeout, forcing close with {} remaining connections",
                    remaining
                );
            }
        }
    } else {
        tracing::info!("No active connections to drain");
    }

    Ok(())
}

async fn accept_loop(
    listener: TcpListener,
    tls_acceptor: Option<TlsAcceptor>,
    semaphore: Arc<Semaphore>,
    shutdown: Arc<Notify>,
) {
    loop {
        tokio::select! {
            result = listener.accept() => {
                match result {
                    Ok((stream, addr)) => {
                        let permit = semaphore.clone().acquire_owned().await;
                        match permit {
                            Ok(permit) => {
                                let tls_acceptor = tls_acceptor.clone();
                                tokio::spawn(async move {
                                    tracing::info!("New connection from {}", addr);

                                    // Perform TLS handshake if configured
                                    let proxy_stream = match tls_acceptor {
                                        Some(acceptor) => {
                                            match acceptor.accept(stream).await {
                                                Ok(tls_stream) => {
                                                    tracing::debug!("TLS handshake completed for {}", addr);
                                                    ProxyStream::Tls(Box::new(tls_stream))
                                                }
                                                Err(e) => {
                                                    tracing::error!("TLS handshake failed for {}: {}", addr, e);
                                                    drop(permit);
                                                    return;
                                                }
                                            }
                                        }
                                        None => ProxyStream::Plain(stream),
                                    };

                                    handle_connection(proxy_stream, addr).await;
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

async fn handle_connection(stream: ProxyStream, addr: std::net::SocketAddr) {
    // For TLS streams, we can't peek to detect protocol
    // We'll need to read the first few bytes instead
    let mut buf = [0u8; 8];

    let protocol_detected = match &stream {
        ProxyStream::Plain { .. } => {
            // For plain streams, use peek
            match stream.peek(&mut buf).await {
                Ok(0) => {
                    tracing::debug!("Connection from {} closed before sending data", addr);
                    return;
                }
                Ok(_) => detect_protocol(&buf),
                Err(e) => {
                    tracing::error!("Failed to peek from {}: {}", addr, e);
                    return;
                }
            }
        }
        ProxyStream::Tls { .. } => {
            // For TLS streams, we need to assume a protocol or use SNI
            // For now, we'll default to HTTP since HTTPS is common
            // TODO: Could be improved with SNI parsing or configuration
            DetectedProtocol::Http
        }
    };

    match protocol_detected {
        DetectedProtocol::Http => {
            tracing::debug!("HTTP protocol detected from {}", addr);
            handle_http_with_stream(stream).await;
        }
        DetectedProtocol::Socks5 => {
            tracing::debug!("SOCKS5 protocol detected from {}", addr);
            handle_socks5_with_stream(stream, addr, None).await;
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

async fn handle_http_with_stream(stream: ProxyStream) {
    match stream {
        ProxyStream::Plain(stream) => {
            http_handler::handle_http(stream).await;
        }
        ProxyStream::Tls(stream) => {
            // For TLS streams, we need to convert to TokioIo
            // The hyper library can work with any AsyncRead + AsyncWrite + Unpin
            http_handler::handle_http_generic(stream).await;
        }
    }
}

async fn handle_socks5_with_stream(
    stream: ProxyStream,
    addr: std::net::SocketAddr,
    users: Option<Vec<crate::config::UserCredential>>,
) {
    match stream {
        ProxyStream::Plain(stream) => {
            socks5::handle_socks5(stream, addr, users).await;
        }
        ProxyStream::Tls(_) => {
            // For now, TLS SOCKS5 is not implemented
            // This would require updating the entire SOCKS5 handler chain to work with generic streams
            tracing::warn!(
                "TLS SOCKS5 not yet implemented for connection from {}",
                addr
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_load_tls_config_missing_files() {
        let config = TlsConfig {
            cert: "/nonexistent/cert.pem".to_string(),
            key: "/nonexistent/key.pem".to_string(),
        };

        let result = load_tls_acceptor(&config);
        assert!(result.is_err());
        if let Err(error) = result {
            assert!(error.to_string().contains("Failed to open cert file"));
        }
    }

    #[test]
    fn test_load_tls_config_invalid_cert() {
        let mut cert_file = NamedTempFile::new().unwrap();
        let mut key_file = NamedTempFile::new().unwrap();

        // Write invalid cert content
        writeln!(cert_file, "-----BEGIN CERTIFICATE-----").unwrap();
        writeln!(cert_file, "invalid cert content").unwrap();
        writeln!(cert_file, "-----END CERTIFICATE-----").unwrap();

        // Write invalid key content
        writeln!(key_file, "-----BEGIN PRIVATE KEY-----").unwrap();
        writeln!(key_file, "invalid key content").unwrap();
        writeln!(key_file, "-----END PRIVATE KEY-----").unwrap();

        let config = TlsConfig {
            cert: cert_file.path().to_str().unwrap().to_string(),
            key: key_file.path().to_str().unwrap().to_string(),
        };

        let result = load_tls_acceptor(&config);
        assert!(result.is_err());
        // Should fail during certificate parsing or TLS config building
    }

    #[test]
    fn test_load_tls_config_empty_files() {
        let cert_file = NamedTempFile::new().unwrap();
        let key_file = NamedTempFile::new().unwrap();

        let config = TlsConfig {
            cert: cert_file.path().to_str().unwrap().to_string(),
            key: key_file.path().to_str().unwrap().to_string(),
        };

        let result = load_tls_acceptor(&config);
        assert!(result.is_err());
        if let Err(error) = result {
            assert!(error.to_string().contains("No certificates found"));
        }
    }

    #[tokio::test]
    async fn test_config_hot_reload() {
        use std::io::Write;
        use tempfile::NamedTempFile;
        use tokio::sync::watch;

        // Create a temporary config file
        let mut config_file = NamedTempFile::new().unwrap();
        let initial_config_content = r#"
server:
  listen:
    - addr: "127.0.0.1:8080"
      protocol: http
  max_connections: 100

access_control:
  default_action: allow
  rules: []

upstream:
  type: direct

logging:
  level: "info"
"#;
        config_file
            .write_all(initial_config_content.as_bytes())
            .unwrap();
        config_file.flush().unwrap();

        let config_path = config_file.path().to_str().unwrap();

        // Load initial config
        let initial_config = AppConfig::load_from_file(config_path).unwrap();
        assert_eq!(initial_config.server.max_connections, Some(100));

        // Test the reload functionality
        let reloaded_config = AppConfig::reload(config_path).unwrap();
        assert_eq!(reloaded_config.server.max_connections, Some(100));

        // Test watch channel for config distribution
        let (config_tx, config_rx) = watch::channel(initial_config.clone());

        // Simulate config reload by sending new config through channel
        let updated_config = AppConfig::reload(config_path).unwrap();
        config_tx.send(updated_config).unwrap();

        // Verify the watch channel received the update
        let received_config = config_rx.borrow().clone();
        assert_eq!(received_config.server.max_connections, Some(100));
        assert_eq!(received_config.logging.level, "info");
    }
}
