use ipnet::IpNet;
use std::net::IpAddr;

/// Check if an IP address matches any of the given CIDR networks
pub fn ip_matches(addr: &IpAddr, networks: &[IpNet]) -> bool {
    networks.iter().any(|net| net.contains(addr))
}
