use thiserror::Error;

#[derive(Error, Debug)]
pub enum ProxyError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Hyper error: {0}")]
    Hyper(#[from] hyper::Error),

    #[error("Config error: {0}")]
    Config(String),

    #[error("ACL denied: {0}")]
    AclDenied(String),

    #[error("Auth required")]
    AuthRequired,

    #[error("Auth failed")]
    AuthFailed,

    #[error("Upstream error: {0}")]
    Upstream(String),

    #[error("Protocol error: {0}")]
    Protocol(String),

    #[error("Timeout")]
    Timeout,

    #[error("SOCKS5 error: {0}")]
    Socks5(String),
}

pub type ProxyResult<T> = Result<T, ProxyError>;
