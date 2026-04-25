use crate::error::{ProxyError, ProxyResult};
use crate::protocol::socks5::handshake::{SocksRequest, TargetAddr};
use dashmap::DashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::AsyncReadExt;
use tokio::net::{TcpStream, UdpSocket};

/// Handle UDP ASSOCIATE command
pub async fn handle_udp_associate(
    tcp_stream: &mut TcpStream,
    _request: &SocksRequest,
) -> ProxyResult<()> {
    // 1. Bind a UDP socket on 0.0.0.0:0 (let OS assign port)
    let udp_socket = UdpSocket::bind("0.0.0.0:0").await.map_err(ProxyError::Io)?;
    let relay_addr = udp_socket.local_addr().map_err(ProxyError::Io)?;

    tracing::debug!("UDP ASSOCIATE: bound relay socket on {}", relay_addr);

    // 2. Send reply with relay address
    send_udp_reply(tcp_stream, 0x00, relay_addr).await?;

    // 3. Run relay loop until TCP connection closes
    relay_loop(tcp_stream, Arc::new(udp_socket)).await
}

/// Send UDP ASSOCIATE reply with actual BND.ADDR:BND.PORT
async fn send_udp_reply(stream: &mut TcpStream, rep: u8, addr: SocketAddr) -> ProxyResult<()> {
    use tokio::io::AsyncWriteExt;

    let mut reply = Vec::new();
    reply.push(0x05); // VER
    reply.push(rep); // REP
    reply.push(0x00); // RSV

    match addr {
        SocketAddr::V4(v4) => {
            reply.push(0x01); // ATYP (IPv4)
            reply.extend_from_slice(&v4.ip().octets());
            reply.extend_from_slice(&v4.port().to_be_bytes());
        }
        SocketAddr::V6(v6) => {
            reply.push(0x04); // ATYP (IPv6)
            reply.extend_from_slice(&v6.ip().octets());
            reply.extend_from_slice(&v6.port().to_be_bytes());
        }
    }

    stream.write_all(&reply).await.map_err(ProxyError::Io)?;
    tracing::debug!("Sent UDP ASSOCIATE reply: rep={}, addr={}", rep, addr);

    Ok(())
}

/// Main relay loop that handles UDP packets and monitors TCP control channel
async fn relay_loop(tcp_stream: &mut TcpStream, udp_socket: Arc<UdpSocket>) -> ProxyResult<()> {
    let mut tcp_buf = [0u8; 1]; // Just to detect TCP connection closure

    // Map client addresses to their target sockets for response routing
    let client_map: Arc<DashMap<SocketAddr, Arc<UdpSocket>>> = Arc::new(DashMap::new());

    loop {
        let mut buf = vec![0u8; 65535]; // Create fresh buffer each iteration

        tokio::select! {
            // Monitor TCP connection (when it closes, we stop)
            result = tcp_stream.read(&mut tcp_buf) => {
                match result {
                    Ok(0) | Err(_) => {
                        tracing::debug!("TCP control channel closed, stopping UDP relay");
                        break;
                    }
                    Ok(_) => {
                        // Ignore any TCP data - this is just a control channel
                        tracing::trace!("Received data on TCP control channel (ignored)");
                    }
                }
            }
            // Handle UDP datagrams
            result = udp_socket.recv_from(&mut buf) => {
                match result {
                    Ok((n, from_addr)) => {
                        let client_map_clone = client_map.clone();
                        let udp_socket_clone = udp_socket.clone();
                        let packet_data = buf[..n].to_vec(); // Clone the data

                        // Process packet in background to avoid blocking the loop
                        tokio::spawn(async move {
                            if let Err(e) = handle_udp_packet(udp_socket_clone, &packet_data, from_addr, client_map_clone).await {
                                tracing::error!("Error handling UDP packet from {}: {}", from_addr, e);
                            }
                        });
                    }
                    Err(e) => {
                        tracing::error!("UDP recv error: {}", e);
                        break;
                    }
                }
            }
        }
    }

    tracing::debug!("UDP relay loop exited");
    Ok(())
}

/// Handle a single UDP packet from a client
async fn handle_udp_packet(
    relay_socket: Arc<UdpSocket>,
    packet_data: &[u8],
    client_addr: SocketAddr,
    client_map: Arc<DashMap<SocketAddr, Arc<UdpSocket>>>,
) -> ProxyResult<()> {
    // Parse SOCKS5 UDP packet header
    let (target_addr, payload) = parse_socks5_udp_packet(packet_data)?;

    tracing::debug!(
        "UDP packet from {} to {:?}, payload len: {}",
        client_addr,
        target_addr,
        payload.len()
    );

    // Create or get target socket for this client
    let target_socket = match client_map.get(&client_addr) {
        Some(socket) => socket.value().clone(),
        None => {
            // Create new target socket
            let target_socket =
                Arc::new(UdpSocket::bind("0.0.0.0:0").await.map_err(ProxyError::Io)?);
            client_map.insert(client_addr, target_socket.clone());

            // Start response listener for this client
            let relay_socket_clone = relay_socket.clone();
            let client_map_clone = client_map.clone();
            let target_addr_clone = target_addr.clone();
            let target_socket_clone = target_socket.clone();
            tokio::spawn(async move {
                listen_for_responses(
                    relay_socket_clone,
                    target_socket_clone,
                    client_addr,
                    target_addr_clone,
                    client_map_clone,
                )
                .await;
            });

            target_socket
        }
    };

    // Forward payload to target
    let target_socket_addr = resolve_target_addr(&target_addr).await?;
    let _ = target_socket
        .send_to(payload, target_socket_addr)
        .await
        .map_err(ProxyError::Io)?;

    Ok(())
}

/// Parse SOCKS5 UDP packet format
/// Returns (target_addr, payload)
fn parse_socks5_udp_packet(data: &[u8]) -> ProxyResult<(TargetAddr, &[u8])> {
    if data.len() < 7 {
        return Err(ProxyError::Socks5("UDP packet too short".into()));
    }

    let mut offset = 0;

    // RSV (2 bytes) - must be 0x0000
    if data[0] != 0x00 || data[1] != 0x00 {
        return Err(ProxyError::Socks5("Invalid RSV field".into()));
    }
    offset += 2;

    // FRAG (1 byte) - must be 0x00 (we don't support fragmentation)
    if data[2] != 0x00 {
        return Err(ProxyError::Socks5("Fragmentation not supported".into()));
    }
    offset += 1;

    // ATYP (1 byte)
    let atyp = data[3];
    offset += 1;

    // Parse address based on ATYP
    let target_addr = match atyp {
        0x01 => {
            // IPv4
            if data.len() < offset + 6 {
                return Err(ProxyError::Socks5("IPv4 address incomplete".into()));
            }
            let ip = [
                data[offset],
                data[offset + 1],
                data[offset + 2],
                data[offset + 3],
            ];
            let port = u16::from_be_bytes([data[offset + 4], data[offset + 5]]);
            offset += 6;
            TargetAddr::Ipv4(ip, port)
        }
        0x03 => {
            // Domain
            if data.len() < offset + 1 {
                return Err(ProxyError::Socks5("Domain length missing".into()));
            }
            let len = data[offset] as usize;
            offset += 1;

            if data.len() < offset + len + 2 {
                return Err(ProxyError::Socks5("Domain name incomplete".into()));
            }
            let domain_bytes = &data[offset..offset + len];
            let domain = String::from_utf8(domain_bytes.to_vec())
                .map_err(|_| ProxyError::Socks5("Invalid domain encoding".into()))?;
            offset += len;

            let port = u16::from_be_bytes([data[offset], data[offset + 1]]);
            offset += 2;
            TargetAddr::Domain(domain, port)
        }
        0x04 => {
            // IPv6
            if data.len() < offset + 18 {
                return Err(ProxyError::Socks5("IPv6 address incomplete".into()));
            }
            let mut ip = [0u8; 16];
            ip.copy_from_slice(&data[offset..offset + 16]);
            let port = u16::from_be_bytes([data[offset + 16], data[offset + 17]]);
            offset += 18;
            TargetAddr::Ipv6(ip, port)
        }
        _ => {
            return Err(ProxyError::Socks5(format!(
                "Unknown address type: {}",
                atyp
            )))
        }
    };

    // Remaining data is payload
    let payload = &data[offset..];
    Ok((target_addr, payload))
}

/// Listen for responses from target and forward back to client
async fn listen_for_responses(
    relay_socket: Arc<UdpSocket>,
    target_socket: Arc<UdpSocket>,
    client_addr: SocketAddr,
    target_addr: TargetAddr,
    client_map: Arc<DashMap<SocketAddr, Arc<UdpSocket>>>,
) {
    let mut buf = vec![0u8; 65535];

    loop {
        match target_socket.recv_from(&mut buf).await {
            Ok((n, _response_from)) => {
                // Build SOCKS5 UDP response packet
                if let Ok(response_packet) = build_socks5_udp_response(&target_addr, &buf[..n]) {
                    if let Err(e) = relay_socket.send_to(&response_packet, client_addr).await {
                        tracing::error!("Failed to send UDP response to {}: {}", client_addr, e);
                        break;
                    }
                }
            }
            Err(e) => {
                tracing::debug!("Target socket recv error: {}", e);
                break;
            }
        }
    }

    // Clean up client mapping when done
    client_map.remove(&client_addr);
    tracing::debug!("Cleaned up response listener for client {}", client_addr);
}

/// Build SOCKS5 UDP response packet
fn build_socks5_udp_response(source_addr: &TargetAddr, payload: &[u8]) -> ProxyResult<Vec<u8>> {
    let mut packet = Vec::new();

    // RSV (2 bytes)
    packet.extend_from_slice(&[0x00, 0x00]);

    // FRAG (1 byte)
    packet.push(0x00);

    // ATYP + ADDR + PORT
    match source_addr {
        TargetAddr::Ipv4(ip, port) => {
            packet.push(0x01); // ATYP
            packet.extend_from_slice(ip);
            packet.extend_from_slice(&port.to_be_bytes());
        }
        TargetAddr::Domain(domain, port) => {
            packet.push(0x03); // ATYP
            packet.push(domain.len() as u8);
            packet.extend_from_slice(domain.as_bytes());
            packet.extend_from_slice(&port.to_be_bytes());
        }
        TargetAddr::Ipv6(ip, port) => {
            packet.push(0x04); // ATYP
            packet.extend_from_slice(ip);
            packet.extend_from_slice(&port.to_be_bytes());
        }
    }

    // Payload
    packet.extend_from_slice(payload);

    Ok(packet)
}

/// Resolve TargetAddr to SocketAddr
async fn resolve_target_addr(target: &TargetAddr) -> ProxyResult<SocketAddr> {
    match target {
        TargetAddr::Ipv4(ip, port) => {
            let addr = std::net::Ipv4Addr::from(*ip);
            Ok(SocketAddr::V4(std::net::SocketAddrV4::new(addr, *port)))
        }
        TargetAddr::Ipv6(ip, port) => {
            let addr = std::net::Ipv6Addr::from(*ip);
            Ok(SocketAddr::V6(std::net::SocketAddrV6::new(
                addr, *port, 0, 0,
            )))
        }
        TargetAddr::Domain(domain, port) => {
            // Use tokio's DNS resolver
            let addr_str = format!("{}:{}", domain, port);
            let mut addrs = tokio::net::lookup_host(&addr_str)
                .await
                .map_err(ProxyError::Io)?;

            addrs
                .next()
                .ok_or_else(|| ProxyError::Socks5(format!("Failed to resolve domain: {}", domain)))
        }
    }
}
