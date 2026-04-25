pub mod direct;
pub mod http_proxy;

use async_trait::async_trait;
use tokio::net::TcpStream;
use std::sync::Arc;
use crate::config::UpstreamConfig;

/// Represents a target to connect to
pub enum ConnectTarget {
    /// Connect to host:port (domain or IP)
    Address(String, u16),
}

/// Trait for upstream connectors
#[async_trait]
pub trait UpstreamConnector: Send + Sync {
    /// Establish a TCP connection to the target
    async fn connect(&self, target: &ConnectTarget) -> crate::error::ProxyResult<TcpStream>;
}

/// Router that selects the appropriate upstream connector based on configuration
pub struct UpstreamRouter {
    connector: Arc<dyn UpstreamConnector>,
}

impl UpstreamRouter {
    pub fn from_config(config: &UpstreamConfig) -> Self {
        let connector: Arc<dyn UpstreamConnector> = match config {
            UpstreamConfig::Direct => Arc::new(direct::DirectConnector),
            UpstreamConfig::Http { addr, auth } => Arc::new(http_proxy::HttpProxyConnector {
                proxy_addr: addr.clone(),
                auth: auth.clone(),
            }),
            UpstreamConfig::Socks5 { addr: _, auth: _ } => {
                // Will be implemented in Task 15
                tracing::warn!("SOCKS5 upstream not yet implemented, falling back to direct");
                Arc::new(direct::DirectConnector)
            }
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
