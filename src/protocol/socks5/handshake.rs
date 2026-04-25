use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::{timeout, Duration};
use crate::error::{ProxyError, ProxyResult};
use crate::config::UserCredential;
use crate::acl::auth;

pub enum SocksCommand {
    Connect,
    Bind,
    UdpAssociate,
}

#[derive(Debug, Clone)]
pub enum TargetAddr {
    Ipv4([u8; 4], u16),
    Domain(String, u16),
    Ipv6([u8; 16], u16),
}

pub struct SocksRequest {
    pub command: SocksCommand,
    pub target: TargetAddr,
}

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);

/// Phase 1: Negotiate authentication method
pub async fn negotiate_method(stream: &mut TcpStream, users: Option<&[UserCredential]>) -> ProxyResult<u8> {
    // Read: VER | NMETHODS | METHODS...
    let mut header = [0u8; 2];
    timeout(HANDSHAKE_TIMEOUT, stream.read_exact(&mut header))
        .await
        .map_err(|_| ProxyError::Timeout)?
        .map_err(ProxyError::Io)?;
    
    if header[0] != 0x05 {
        return Err(ProxyError::Socks5("Invalid SOCKS version".into()));
    }
    
    let nmethods = header[1] as usize;
    let mut methods = vec![0u8; nmethods];
    timeout(HANDSHAKE_TIMEOUT, stream.read_exact(&mut methods))
        .await
        .map_err(|_| ProxyError::Timeout)?
        .map_err(ProxyError::Io)?;
    
    // Choose method based on configuration
    let selected = if let Some(_) = users {
        // Users are configured, prefer username/password authentication
        if methods.contains(&0x02) {
            0x02 // Username/password authentication
        } else {
            0xFF // No acceptable methods
        }
    } else {
        // No users configured, use no authentication
        if methods.contains(&0x00) {
            0x00 // No authentication
        } else {
            0xFF // No acceptable methods
        }
    };
    
    // Reply: VER | METHOD
    stream.write_all(&[0x05, selected]).await.map_err(ProxyError::Io)?;
    
    if selected == 0xFF {
        return Err(ProxyError::Socks5("No acceptable authentication method".into()));
    }
    
    Ok(selected)
}

/// Phase 1.5: Handle username/password authentication (RFC 1929)
pub async fn authenticate_user(stream: &mut TcpStream, users: &[UserCredential]) -> ProxyResult<()> {
    // Read: VER | ULEN | USERNAME | PLEN | PASSWORD
    let mut ver = [0u8; 1];
    timeout(HANDSHAKE_TIMEOUT, stream.read_exact(&mut ver))
        .await
        .map_err(|_| ProxyError::Timeout)?
        .map_err(ProxyError::Io)?;
    
    if ver[0] != 0x01 {
        return Err(ProxyError::Socks5("Invalid auth version".into()));
    }
    
    // Read username length
    let mut ulen = [0u8; 1];
    timeout(HANDSHAKE_TIMEOUT, stream.read_exact(&mut ulen))
        .await
        .map_err(|_| ProxyError::Timeout)?
        .map_err(ProxyError::Io)?;
    
    // Read username
    let mut username = vec![0u8; ulen[0] as usize];
    timeout(HANDSHAKE_TIMEOUT, stream.read_exact(&mut username))
        .await
        .map_err(|_| ProxyError::Timeout)?
        .map_err(ProxyError::Io)?;
    
    // Read password length
    let mut plen = [0u8; 1];
    timeout(HANDSHAKE_TIMEOUT, stream.read_exact(&mut plen))
        .await
        .map_err(|_| ProxyError::Timeout)?
        .map_err(ProxyError::Io)?;
    
    // Read password
    let mut password = vec![0u8; plen[0] as usize];
    timeout(HANDSHAKE_TIMEOUT, stream.read_exact(&mut password))
        .await
        .map_err(|_| ProxyError::Timeout)?
        .map_err(ProxyError::Io)?;
    
    // Verify credentials
    let username_str = String::from_utf8(username)
        .map_err(|_| ProxyError::Socks5("Invalid username encoding".into()))?;
    let password_str = String::from_utf8(password)
        .map_err(|_| ProxyError::Socks5("Invalid password encoding".into()))?;
    
    let authenticated = auth::verify_credentials(&username_str, &password_str, users);
    
    // Reply: VER | STATUS (0x00=success, 0x01=failure)
    let status = if authenticated { 0x00 } else { 0x01 };
    stream.write_all(&[0x01, status]).await.map_err(ProxyError::Io)?;
    
    if !authenticated {
        return Err(ProxyError::Socks5("Authentication failed".into()));
    }
    
    Ok(())
}

/// Phase 2: Read SOCKS5 request
pub async fn read_request(stream: &mut TcpStream) -> ProxyResult<SocksRequest> {
    // Read: VER | CMD | RSV | ATYP
    let mut header = [0u8; 4];
    timeout(HANDSHAKE_TIMEOUT, stream.read_exact(&mut header))
        .await
        .map_err(|_| ProxyError::Timeout)?
        .map_err(ProxyError::Io)?;
    
    if header[0] != 0x05 {
        return Err(ProxyError::Socks5("Invalid SOCKS version in request".into()));
    }
    
    let command = match header[1] {
        0x01 => SocksCommand::Connect,
        0x02 => SocksCommand::Bind,
        0x03 => SocksCommand::UdpAssociate,
        _ => return Err(ProxyError::Socks5(format!("Unknown command: {}", header[1]))),
    };
    
    // Parse address based on ATYP
    let target = match header[3] {
        0x01 => { // IPv4
            let mut addr = [0u8; 4];
            timeout(HANDSHAKE_TIMEOUT, stream.read_exact(&mut addr))
                .await
                .map_err(|_| ProxyError::Timeout)?
                .map_err(ProxyError::Io)?;
            let mut port_buf = [0u8; 2];
            timeout(HANDSHAKE_TIMEOUT, stream.read_exact(&mut port_buf))
                .await
                .map_err(|_| ProxyError::Timeout)?
                .map_err(ProxyError::Io)?;
            let port = u16::from_be_bytes(port_buf);
            TargetAddr::Ipv4(addr, port)
        }
        0x03 => { // Domain
            let mut len = [0u8; 1];
            timeout(HANDSHAKE_TIMEOUT, stream.read_exact(&mut len))
                .await
                .map_err(|_| ProxyError::Timeout)?
                .map_err(ProxyError::Io)?;
            let mut domain = vec![0u8; len[0] as usize];
            timeout(HANDSHAKE_TIMEOUT, stream.read_exact(&mut domain))
                .await
                .map_err(|_| ProxyError::Timeout)?
                .map_err(ProxyError::Io)?;
            let mut port_buf = [0u8; 2];
            timeout(HANDSHAKE_TIMEOUT, stream.read_exact(&mut port_buf))
                .await
                .map_err(|_| ProxyError::Timeout)?
                .map_err(ProxyError::Io)?;
            let port = u16::from_be_bytes(port_buf);
            let domain_str = String::from_utf8(domain)
                .map_err(|_| ProxyError::Socks5("Invalid domain encoding".into()))?;
            TargetAddr::Domain(domain_str, port)
        }
        0x04 => { // IPv6
            let mut addr = [0u8; 16];
            timeout(HANDSHAKE_TIMEOUT, stream.read_exact(&mut addr))
                .await
                .map_err(|_| ProxyError::Timeout)?
                .map_err(ProxyError::Io)?;
            let mut port_buf = [0u8; 2];
            timeout(HANDSHAKE_TIMEOUT, stream.read_exact(&mut port_buf))
                .await
                .map_err(|_| ProxyError::Timeout)?
                .map_err(ProxyError::Io)?;
            let port = u16::from_be_bytes(port_buf);
            TargetAddr::Ipv6(addr, port)
        }
        _ => return Err(ProxyError::Socks5(format!("Unknown address type: {}", header[3]))),
    };
    
    Ok(SocksRequest { command, target })
}

/// Send SOCKS5 reply
pub async fn send_reply(stream: &mut TcpStream, rep: u8, _request: &SocksRequest) -> ProxyResult<()> {
    // Reply: VER | REP | RSV | ATYP | BND.ADDR | BND.PORT
    // Use 0.0.0.0:0 as bind address
    let reply = [
        0x05, rep, 0x00, 0x01, // VER, REP, RSV, ATYP (IPv4)
        0x00, 0x00, 0x00, 0x00, // BND.ADDR (0.0.0.0)
        0x00, 0x00, // BND.PORT (0)
    ];
    stream.write_all(&reply).await.map_err(ProxyError::Io)?;
    Ok(())
}