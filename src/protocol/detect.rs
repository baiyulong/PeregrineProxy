#[derive(Debug, PartialEq, Eq)]
pub enum DetectedProtocol {
    Http,
    Socks5,
    Unknown,
}

/// Detect the protocol from the first bytes of a connection.
/// The buffer should be at least 8 bytes obtained via peek().
pub fn detect_protocol(buf: &[u8]) -> DetectedProtocol {
    if buf.is_empty() {
        return DetectedProtocol::Unknown;
    }
    
    // SOCKS5: first byte is 0x05
    if buf[0] == 0x05 {
        return DetectedProtocol::Socks5;
    }
    
    // HTTP: check if buffer starts with an HTTP method
    if is_http_method_prefix(buf) {
        return DetectedProtocol::Http;
    }
    
    DetectedProtocol::Unknown
}

fn is_http_method_prefix(buf: &[u8]) -> bool {
    const METHODS: &[&[u8]] = &[
        b"GET ",
        b"POST ",
        b"PUT ",
        b"DELETE ",
        b"HEAD ",
        b"OPTIONS ",
        b"PATCH ",
        b"CONNECT ",
        b"TRACE ",
    ];
    
    for method in METHODS {
        if buf.len() >= method.len() && &buf[..method.len()] == *method {
            return true;
        }
        // Also match if buffer is shorter than method but matches so far
        if buf.len() < method.len() && method.starts_with(buf) {
            return true;
        }
    }
    false
}