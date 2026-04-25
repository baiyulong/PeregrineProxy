use peregrine::config::AppConfig;

#[test]
fn test_load_example_config() {
    // Test loading the example config file
    let config = AppConfig::load_from_file("config.example.yaml");
    assert!(config.is_ok(), "Failed to load example config: {:?}", config.err());
}

#[test]
fn test_load_minimal_config() {
    let yaml = r#"
server:
  listen:
    - addr: "0.0.0.0:8080"
      protocol: http
access_control:
  default_action: allow
  rules: []
upstream:
  type: direct
logging:
  level: info
"#;
    let config: AppConfig = serde_yaml::from_str(yaml).unwrap();
    assert_eq!(config.server.listen.len(), 1);
    assert_eq!(config.server.listen[0].addr, "0.0.0.0:8080");
}

#[test]
fn test_load_full_config() {
    let yaml = r#"
server:
  listen:
    - addr: "0.0.0.0:8080"
      protocol: http
    - addr: "0.0.0.0:1080"
      protocol: socks5
  max_connections: 10000
  tcp_keepalive: 60
  connect_timeout: 10
  read_timeout: 30
  write_timeout: 30
access_control:
  default_action: deny
  rules:
    - action: allow
      src_ip: ["192.168.0.0/16", "10.0.0.0/8"]
    - action: deny
      dst_domain: ["*.blocked.com"]
    - action: authenticate
      src_ip: ["0.0.0.0/0"]
      auth:
        type: basic
        users:
          - username: admin
            password: secret
upstream:
  type: http
  addr: "parent-proxy.example.com:3128"
  auth:
    username: proxyuser
    password: proxypass
logging:
  level: info
  access_log: "./access.log"
  format: json
metrics:
  enabled: true
  listen: "127.0.0.1:9090"
"#;
    let config: AppConfig = serde_yaml::from_str(yaml).unwrap();
    assert_eq!(config.server.max_connections, Some(10000));
    assert_eq!(config.access_control.rules.len(), 3);
    assert!(config.metrics.as_ref().unwrap().enabled);
}

#[test]
fn test_default_values() {
    let yaml = r#"
server:
  listen:
    - addr: "0.0.0.0:8080"
      protocol: http
access_control:
  default_action: allow
  rules: []
upstream:
  type: direct
logging:
  level: info
"#;
    let config: AppConfig = serde_yaml::from_str(yaml).unwrap();
    assert_eq!(config.server.max_connections, None);
    assert_eq!(config.server.tcp_keepalive, None);
}