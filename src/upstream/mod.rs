pub mod direct;

use async_trait::async_trait;
use tokio::net::TcpStream;

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
