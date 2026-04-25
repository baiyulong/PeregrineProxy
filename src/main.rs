pub mod config;
mod error;
mod protocol;
mod server;

use clap::Parser;
use config::{AppConfig, CliArgs};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize basic logging
    tracing_subscriber::fmt::init();
    
    let args = CliArgs::parse();
    let config = AppConfig::load_from_file(&args.config)?;
    
    tracing::info!("Peregrine proxy starting...");
    tracing::info!("Loaded config from: {}", args.config);
    
    server::run(config).await?;
    
    Ok(())
}
