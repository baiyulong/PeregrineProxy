use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct AppConfig {
    pub server: ServerConfig,
    pub access_control: AccessControlConfig,
    pub upstream: UpstreamConfig,
    pub logging: LoggingConfig,
    pub metrics: Option<MetricsConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ServerConfig {
    pub listen: Vec<ListenConfig>,
    pub max_connections: Option<usize>,
    pub tcp_keepalive: Option<u64>,
    pub connect_timeout: Option<u64>,
    pub read_timeout: Option<u64>,
    pub write_timeout: Option<u64>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ListenConfig {
    pub addr: String,
    pub protocol: Protocol,
    pub tls: Option<TlsConfig>,
}

#[derive(Debug, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    Http,
    Socks5,
}

#[derive(Debug, Deserialize, Clone)]
pub struct TlsConfig {
    pub cert: String,
    pub key: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct AccessControlConfig {
    pub default_action: AclActionConfig,
    pub rules: Vec<AclRuleConfig>,
}

#[derive(Debug, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum AclActionConfig {
    Allow,
    Deny,
    Authenticate,
}

#[derive(Debug, Deserialize, Clone)]
pub struct AclRuleConfig {
    pub action: AclActionConfig,
    pub src_ip: Option<Vec<String>>,
    pub dst_domain: Option<Vec<String>>,
    pub dst_port: Option<Vec<String>>,
    pub http_method: Option<Vec<String>>,
    pub auth: Option<AuthConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct AuthConfig {
    #[serde(rename = "type")]
    pub auth_type: String,
    pub users: Option<Vec<UserCredential>>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct UserCredential {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum UpstreamConfig {
    Direct,
    Http {
        addr: String,
        auth: Option<UserCredential>,
    },
    Socks5 {
        addr: String,
        auth: Option<UserCredential>,
    },
    Chain {
        chain: Vec<UpstreamConfig>,
    },
}

#[derive(Debug, Deserialize, Clone)]
pub struct LoggingConfig {
    pub level: String,
    pub access_log: Option<String>,
    pub format: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct MetricsConfig {
    pub enabled: bool,
    pub listen: String,
}

impl AppConfig {
    pub fn load_from_file(path: &str) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: AppConfig = serde_yaml::from_str(&content)?;
        Ok(config)
    }
}