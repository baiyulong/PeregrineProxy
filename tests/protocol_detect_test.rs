use peregrine::protocol::detect::{detect_protocol, DetectedProtocol};

#[test]
fn test_detect_socks5() {
    let buf = [0x05, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
    assert_eq!(detect_protocol(&buf), DetectedProtocol::Socks5);
}

#[test]
fn test_detect_http_get() {
    let buf = b"GET / HT";
    assert_eq!(detect_protocol(buf), DetectedProtocol::Http);
}

#[test]
fn test_detect_http_connect() {
    let buf = b"CONNECT ";
    assert_eq!(detect_protocol(buf), DetectedProtocol::Http);
}

#[test]
fn test_detect_http_post() {
    let buf = b"POST / H";
    assert_eq!(detect_protocol(buf), DetectedProtocol::Http);
}

#[test]
fn test_detect_http_put() {
    let buf = b"PUT / HT";
    assert_eq!(detect_protocol(buf), DetectedProtocol::Http);
}

#[test]
fn test_detect_http_delete() {
    let buf = b"DELETE /";
    assert_eq!(detect_protocol(buf), DetectedProtocol::Http);
}

#[test]
fn test_detect_http_head() {
    let buf = b"HEAD / H";
    assert_eq!(detect_protocol(buf), DetectedProtocol::Http);
}

#[test]
fn test_detect_http_options() {
    let buf = b"OPTIONS ";
    assert_eq!(detect_protocol(buf), DetectedProtocol::Http);
}

#[test]
fn test_detect_http_patch() {
    let buf = b"PATCH / ";
    assert_eq!(detect_protocol(buf), DetectedProtocol::Http);
}

#[test]
fn test_detect_unknown() {
    let buf = [0xFF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
    assert_eq!(detect_protocol(&buf), DetectedProtocol::Unknown);
}

#[test]
fn test_detect_empty_buffer() {
    let buf = [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
    assert_eq!(detect_protocol(&buf), DetectedProtocol::Unknown);
}
