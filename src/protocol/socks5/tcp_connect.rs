use tokio::net::TcpStream;
use crate::protocol::socks5::handshake::{SocksRequest, TargetAddr, send_reply};
use crate::upstream::direct::DirectConnector;
use crate::upstream::{ConnectTarget, UpstreamConnector};
use crate::error::ProxyResult;

pub async fn handle_connect(stream: &mut TcpStream, request: &SocksRequest) -> ProxyResult<()> {
    let (host, port) = match &request.target {
        TargetAddr::Ipv4(addr, port) => {
            let ip = std::net::Ipv4Addr::new(addr[0], addr[1], addr[2], addr[3]);
            (ip.to_string(), *port)
        }
        TargetAddr::Domain(domain, port) => (domain.clone(), *port),
        TargetAddr::Ipv6(addr, port) => {
            let ip = std::net::Ipv6Addr::from(*addr);
            (ip.to_string(), *port)
        }
    };
    
    tracing::info!("SOCKS5 CONNECT to {}:{}", host, port);
    
    let connector = DirectConnector;
    match connector.connect(&ConnectTarget::Address(host.clone(), port)).await {
        Ok(mut target_stream) => {
            // Send success reply
            send_reply(stream, 0x00, request).await?;
            
            // Bidirectional copy
            match tokio::io::copy_bidirectional(stream, &mut target_stream).await {
                Ok((from_client, from_server)) => {
                    tracing::debug!(
                        "SOCKS5 tunnel to {}:{} closed: {} bytes from client, {} bytes from server",
                        host, port, from_client, from_server
                    );
                }
                Err(e) => {
                    tracing::debug!("SOCKS5 tunnel error: {}", e);
                }
            }
        }
        Err(e) => {
            tracing::error!("SOCKS5 connect to {}:{} failed: {}", host, port, e);
            // Send connection refused reply
            send_reply(stream, 0x05, request).await?;
        }
    }
    
    Ok(())
}