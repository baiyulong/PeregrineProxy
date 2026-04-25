use async_trait::async_trait;
use tokio::net::TcpStream;
use crate::upstream::{ConnectTarget, UpstreamConnector, BoxedStream};
use crate::error::{ProxyError, ProxyResult};

pub struct DirectConnector;

#[async_trait]
impl UpstreamConnector for DirectConnector {
    async fn connect(&self, target: &ConnectTarget) -> ProxyResult<BoxedStream> {
        match target {
            ConnectTarget::Address(host, port) => {
                let addr = format!("{}:{}", host, port);
                let stream = TcpStream::connect(&addr).await.map_err(ProxyError::Io)?;
                Ok(Box::pin(stream) as BoxedStream)
            }
        }
    }
}
