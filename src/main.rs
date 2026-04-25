mod acl;
pub mod config;
mod error;
mod logging;
mod metrics;
mod protocol;
mod server;
mod upstream;

use clap::Parser;
use config::{AppConfig, CliArgs};

#[tokio::main]
async fn main() {
    let args = CliArgs::parse();

    let config = match AppConfig::load_from_file(&args.config) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error: failed to load config '{}': {}", args.config, e);
            std::process::exit(1);
        }
    };

    // Initialize logging from config (with CLI override)
    let mut log_config = config.logging.clone();
    if let Some(ref level) = args.log_level {
        log_config.level = level.clone();
    }
    if let Err(e) = logging::init_logging(&log_config) {
        eprintln!("Error: failed to initialize logging: {}", e);
        std::process::exit(1);
    }

    tracing::info!("Peregrine proxy starting...");
    tracing::info!("Loaded config from: {}", args.config);

    if let Err(e) = server::run(config, &args.config).await {
        tracing::error!("Server error: {}", e);
        std::process::exit(1);
    }
}
