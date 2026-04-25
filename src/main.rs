pub mod config;
mod acl;
mod error;
mod logging;
mod protocol;
mod server;
mod upstream;

use clap::Parser;
use config::{AppConfig, CliArgs};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = CliArgs::parse();
    let config = AppConfig::load_from_file(&args.config)?;

    // Initialize logging from config (with CLI override)
    let mut log_config = config.logging.clone();
    if let Some(ref level) = args.log_level {
        log_config.level = level.clone();
    }
    logging::init_logging(&log_config)?;

    tracing::info!("Peregrine proxy starting...");
    tracing::info!("Loaded config from: {}", args.config);

    server::run(config).await?;

    Ok(())
}
