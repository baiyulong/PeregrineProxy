pub mod config;
mod error;

use clap::Parser;
use config::{AppConfig, CliArgs};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = CliArgs::parse();
    
    let config = AppConfig::load_from_file(&args.config)?;
    
    println!("Peregrine proxy starting...");
    println!("Loaded config from: {}", args.config);
    println!("Listening on {} address(es)", config.server.listen.len());
    
    Ok(())
}
