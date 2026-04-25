use crate::config::UserCredential;
use crate::error::{ProxyError, ProxyResult};
use crate::upstream::{extract_host, wrap_tls, BoxedStream, ConnectTarget, UpstreamConnector};
use async_trait::async_trait;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

pub struct Socks5ProxyConnector {
    pub proxy_addr: String,
    pub auth: Option<UserCredential>,
    pub use_tls: bool,
}

#[async_trait]
impl UpstreamConnector for Socks5ProxyConnector {
    async fn connect(&self, target: &ConnectTarget) -> ProxyResult<BoxedStream> {
        let stream = TcpStream::connect(&self.proxy_addr).await.map_err(|e| {
            ProxyError::Upstream(format!(
                "Failed to connect to SOCKS5 proxy {}: {}",
                self.proxy_addr, e
            ))
        })?;

        if self.use_tls {
            let host = extract_host(&self.proxy_addr);
            let tls_stream = wrap_tls(stream, &host).await?;
            let mut tls_stream = Box::pin(tls_stream);

            // Perform SOCKS5 handshake through TLS
            self.perform_socks5_handshake(&mut tls_stream, target)
                .await?;
            Ok(tls_stream as BoxedStream)
        } else {
            let mut stream = stream;

            // Perform SOCKS5 handshake on plain TCP
            self.perform_socks5_handshake(&mut stream, target).await?;
            Ok(Box::pin(stream) as BoxedStream)
        }
    }
}

impl Socks5ProxyConnector {
    async fn perform_socks5_handshake<S>(
        &self,
        stream: &mut S,
        target: &ConnectTarget,
    ) -> ProxyResult<()>
    where
        S: AsyncReadExt + AsyncWriteExt + Unpin,
    {
        // Phase 1: Method negotiation
        let methods = if self.auth.is_some() {
            vec![0x05, 0x02, 0x00, 0x02] // VER, NMETHODS=2, NO_AUTH + USERNAME_PASSWORD
        } else {
            vec![0x05, 0x01, 0x00] // VER, NMETHODS=1, NO_AUTH
        };
        stream.write_all(&methods).await.map_err(ProxyError::Io)?;

        let mut resp = [0u8; 2];
        stream.read_exact(&mut resp).await.map_err(ProxyError::Io)?;

        if resp[0] != 0x05 {
            return Err(ProxyError::Socks5(
                "Invalid SOCKS5 version from upstream".into(),
            ));
        }

        match resp[1] {
            0x00 => {} // No auth required
            0x02 => {
                // Username/password auth (RFC 1929)
                let auth = self.auth.as_ref().ok_or_else(|| {
                    ProxyError::Socks5(
                        "Upstream requires auth but no credentials configured".into(),
                    )
                })?;

                let mut auth_req = vec![0x01]; // VER
                auth_req.push(auth.username.len() as u8);
                auth_req.extend_from_slice(auth.username.as_bytes());
                auth_req.push(auth.password.len() as u8);
                auth_req.extend_from_slice(auth.password.as_bytes());
                stream.write_all(&auth_req).await.map_err(ProxyError::Io)?;

                let mut auth_resp = [0u8; 2];
                stream
                    .read_exact(&mut auth_resp)
                    .await
                    .map_err(ProxyError::Io)?;

                if auth_resp[1] != 0x00 {
                    return Err(ProxyError::AuthFailed);
                }
            }
            0xFF => {
                return Err(ProxyError::Socks5(
                    "Upstream SOCKS5 proxy rejected all auth methods".into(),
                ))
            }
            other => {
                return Err(ProxyError::Socks5(format!(
                    "Unsupported auth method from upstream: {}",
                    other
                )))
            }
        }

        // Phase 2: CONNECT request
        match target {
            ConnectTarget::Address(host, port) => {
                let mut req = vec![0x05, 0x01, 0x00]; // VER, CMD=CONNECT, RSV

                // Try to parse as IP, otherwise use domain
                if let Ok(ipv4) = host.parse::<std::net::Ipv4Addr>() {
                    req.push(0x01); // ATYP = IPv4
                    req.extend_from_slice(&ipv4.octets());
                } else if let Ok(ipv6) = host.parse::<std::net::Ipv6Addr>() {
                    req.push(0x04); // ATYP = IPv6
                    req.extend_from_slice(&ipv6.octets());
                } else {
                    req.push(0x03); // ATYP = Domain
                    req.push(host.len() as u8);
                    req.extend_from_slice(host.as_bytes());
                }
                req.extend_from_slice(&port.to_be_bytes());

                stream.write_all(&req).await.map_err(ProxyError::Io)?;

                // Read reply header: VER | REP | RSV | ATYP
                let mut reply_header = [0u8; 4];
                stream
                    .read_exact(&mut reply_header)
                    .await
                    .map_err(ProxyError::Io)?;

                if reply_header[1] != 0x00 {
                    let err_msg = match reply_header[1] {
                        0x01 => "general SOCKS server failure",
                        0x02 => "connection not allowed by ruleset",
                        0x03 => "network unreachable",
                        0x04 => "host unreachable",
                        0x05 => "connection refused",
                        0x06 => "TTL expired",
                        0x07 => "command not supported",
                        0x08 => "address type not supported",
                        _ => "unknown error",
                    };
                    return Err(ProxyError::Upstream(format!(
                        "SOCKS5 upstream CONNECT failed: {}",
                        err_msg
                    )));
                }

                // Read and discard bind address based on ATYP
                match reply_header[3] {
                    0x01 => {
                        // IPv4: 4 bytes + 2 port
                        let mut buf = [0u8; 6];
                        stream.read_exact(&mut buf).await.map_err(ProxyError::Io)?;
                    }
                    0x03 => {
                        // Domain: 1 len + domain + 2 port
                        let mut len = [0u8; 1];
                        stream.read_exact(&mut len).await.map_err(ProxyError::Io)?;
                        let mut buf = vec![0u8; len[0] as usize + 2];
                        stream.read_exact(&mut buf).await.map_err(ProxyError::Io)?;
                    }
                    0x04 => {
                        // IPv6: 16 bytes + 2 port
                        let mut buf = [0u8; 18];
                        stream.read_exact(&mut buf).await.map_err(ProxyError::Io)?;
                    }
                    _ => {}
                }

                Ok(())
            }
        }
    }
}
