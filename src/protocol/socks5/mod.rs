pub mod handshake;
pub mod tcp_connect;
pub mod udp_associate;

use crate::config::UserCredential;
use std::net::SocketAddr;
use tokio::net::TcpStream;

pub async fn handle_socks5(
    stream: TcpStream,
    addr: SocketAddr,
    users: Option<Vec<UserCredential>>,
) {
    if let Err(e) = handle_socks5_inner(stream, addr, users).await {
        tracing::error!("SOCKS5 error from {}: {}", addr, e);
    }
}

async fn handle_socks5_inner(
    mut stream: TcpStream,
    _addr: SocketAddr,
    users: Option<Vec<UserCredential>>,
) -> crate::error::ProxyResult<()> {
    // Phase 1: Method negotiation
    let method = handshake::negotiate_method(&mut stream, users.as_deref()).await?;

    // Phase 1.5: Authentication (if required)
    if method == 0x02 {
        if let Some(ref user_list) = users {
            handshake::authenticate_user(&mut stream, user_list).await?;
        } else {
            return Err(crate::error::ProxyError::Socks5(
                "Authentication required but no users configured".into(),
            ));
        }
    }

    // Phase 2: Read request
    let request = handshake::read_request(&mut stream).await?;

    // Phase 3: Handle command
    match request.command {
        handshake::SocksCommand::Connect => {
            tcp_connect::handle_connect(&mut stream, &request).await?;
        }
        handshake::SocksCommand::UdpAssociate => {
            udp_associate::handle_udp_associate(&mut stream, &request).await?;
        }
        _ => {
            handshake::send_reply(&mut stream, 0x07, &request).await?; // Command not supported
        }
    }

    Ok(())
}
