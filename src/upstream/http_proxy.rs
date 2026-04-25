use async_trait::async_trait;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use crate::upstream::{ConnectTarget, UpstreamConnector};
use crate::error::{ProxyError, ProxyResult};
use crate::config::UserCredential;
use base64::Engine;

pub struct HttpProxyConnector {
    pub proxy_addr: String,
    pub auth: Option<UserCredential>,
}

#[async_trait]
impl UpstreamConnector for HttpProxyConnector {
    async fn connect(&self, target: &ConnectTarget) -> ProxyResult<TcpStream> {
        let mut stream = TcpStream::connect(&self.proxy_addr).await
            .map_err(|e| ProxyError::Upstream(format!("Failed to connect to HTTP proxy {}: {}", self.proxy_addr, e)))?;
        
        match target {
            ConnectTarget::Address(host, port) => {
                // Send CONNECT request
                let mut request = format!(
                    "CONNECT {}:{} HTTP/1.1\r\nHost: {}:{}\r\n",
                    host, port, host, port
                );
                
                // Add proxy auth if configured
                if let Some(ref auth) = self.auth {
                    let credentials = base64::engine::general_purpose::STANDARD
                        .encode(format!("{}:{}", auth.username, auth.password));
                    request.push_str(&format!("Proxy-Authorization: Basic {}\r\n", credentials));
                }
                
                request.push_str("\r\n");
                
                stream.write_all(request.as_bytes()).await.map_err(ProxyError::Io)?;
                
                // Read response
                let mut buf = vec![0u8; 4096];
                let n = stream.read(&mut buf).await.map_err(ProxyError::Io)?;
                let response = String::from_utf8_lossy(&buf[..n]);
                
                if !response.starts_with("HTTP/1.1 200") && !response.starts_with("HTTP/1.0 200") {
                    return Err(ProxyError::Upstream(format!(
                        "HTTP proxy CONNECT failed: {}", response.lines().next().unwrap_or("empty")
                    )));
                }
                
                Ok(stream)
            }
        }
    }
}