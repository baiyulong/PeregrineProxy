pub mod direct;
pub mod http_proxy;
pub mod socks5_proxy;

use async_trait::async_trait;
use tokio::net::TcpStream;
use std::sync::Arc;
use std::pin::Pin;
use crate::config::UpstreamConfig;
use crate::error::{ProxyError, ProxyResult};
use tokio_rustls::{TlsConnector, client::TlsStream};
use rustls::ClientConfig;
use rustls::pki_types::ServerName;

/// Represents a target to connect to
pub enum ConnectTarget {
    /// Connect to host:port (domain or IP)
    Address(String, u16),
}

/// Stream trait combining AsyncRead and AsyncWrite
pub trait Stream: tokio::io::AsyncRead + tokio::io::AsyncWrite + Send + Unpin {}

impl<T> Stream for T where T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Send + Unpin {}

/// Boxed stream type that can handle both TLS and plain TCP
pub type BoxedStream = Pin<Box<dyn Stream>>;

/// Create a TLS connector with system root certificates
pub fn create_tls_connector() -> TlsConnector {
    let mut root_store = rustls::RootCertStore::empty();
    // Add webpki roots (standard root certs)
    root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    
    let config = ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();
    
    TlsConnector::from(Arc::new(config))
}

/// Wrap a TCP stream with TLS
pub async fn wrap_tls(stream: TcpStream, host: &str) -> ProxyResult<TlsStream<TcpStream>> {
    let connector = create_tls_connector();
    let server_name = ServerName::try_from(host.to_string())
        .map_err(|_| ProxyError::Upstream(format!("Invalid TLS server name: {}", host)))?;
    connector.connect(server_name, stream).await
        .map_err(|e| ProxyError::Upstream(format!("TLS handshake failed: {}", e)))
}

/// Extract hostname from address string (host:port)
pub fn extract_host(addr: &str) -> String {
    if let Some(colon_pos) = addr.rfind(':') {
        addr[..colon_pos].to_string()
    } else {
        addr.to_string()
    }
}

/// Trait for upstream connectors
#[async_trait]
pub trait UpstreamConnector: Send + Sync {
    /// Establish a connection to the target (may be TLS or plain TCP)
    async fn connect(&self, target: &ConnectTarget) -> crate::error::ProxyResult<BoxedStream>;
}

/// Router that selects the appropriate upstream connector based on configuration
pub struct UpstreamRouter {
    connector: Arc<dyn UpstreamConnector>,
}

impl UpstreamRouter {
    pub fn from_config(config: &UpstreamConfig) -> Self {
        let connector: Arc<dyn UpstreamConnector> = match config {
            UpstreamConfig::Direct => Arc::new(direct::DirectConnector),
            UpstreamConfig::Http { addr, auth, tls } => Arc::new(http_proxy::HttpProxyConnector {
                proxy_addr: addr.clone(),
                auth: auth.clone(),
                use_tls: tls.unwrap_or(false),
            }),
            UpstreamConfig::Socks5 { addr, auth, tls } => Arc::new(socks5_proxy::Socks5ProxyConnector {
                proxy_addr: addr.clone(),
                auth: auth.clone(),
                use_tls: tls.unwrap_or(false),
            }),
            UpstreamConfig::Chain { chain: _ } => {
                // Will be implemented in Task 16
                tracing::warn!("Chain upstream not yet implemented, falling back to direct");
                Arc::new(direct::DirectConnector)
            }
        };
        
        Self { connector }
    }
    
    pub fn connector(&self) -> &dyn UpstreamConnector {
        self.connector.as_ref()
    }
}
