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
use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    // Write to stderr immediately — verifies the binary is executing and is always
    // visible regardless of stdout pipe buffering (especially on Windows/PowerShell).
    eprintln!("Peregrine proxy starting...");

    let args = CliArgs::parse();

    let config = match AppConfig::load_from_file(&args.config) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error: failed to load config '{}': {}", args.config, e);
            return ExitCode::FAILURE;
        }
    };

    eprintln!("Config loaded from: {}", args.config);

    // Initialize logging from config (with CLI override)
    let mut log_config = config.logging.clone();
    if let Some(ref level) = args.log_level {
        log_config.level = level.clone();
    }
    if let Err(e) = logging::init_logging(&log_config) {
        eprintln!("Error: failed to initialize logging: {}", e);
        return ExitCode::FAILURE;
    }

    if let Err(e) = server::run(config, &args.config).await {
        eprintln!("Error: {}", e);
        return ExitCode::FAILURE;
    }

    ExitCode::SUCCESS
}
