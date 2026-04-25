pub mod handshake;
pub mod tcp_connect;

use tokio::net::TcpStream;
use std::net::SocketAddr;

pub async fn handle_socks5(stream: TcpStream, addr: SocketAddr) {
    if let Err(e) = handle_socks5_inner(stream, addr).await {
        tracing::error!("SOCKS5 error from {}: {}", addr, e);
    }
}

async fn handle_socks5_inner(mut stream: TcpStream, _addr: SocketAddr) -> crate::error::ProxyResult<()> {
    // Phase 1: Method negotiation
    let _method = handshake::negotiate_method(&mut stream).await?;
    
    // Phase 2: Read request
    let request = handshake::read_request(&mut stream).await?;
    
    // Phase 3: Handle command
    match request.command {
        handshake::SocksCommand::Connect => {
            tcp_connect::handle_connect(&mut stream, &request).await?;
        }
        _ => {
            handshake::send_reply(&mut stream, 0x07, &request).await?; // Command not supported
        }
    }
    
    Ok(())
}