use crate::config::LoggingConfig;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

pub fn init_logging(config: &LoggingConfig) -> anyhow::Result<()> {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&config.level));

    let format = config.format.as_deref().unwrap_or("text");

    match format {
        "json" => {
            let fmt_layer = fmt::layer()
                .json()
                .with_writer(std::io::stdout)
                .with_target(true)
                .with_thread_ids(true)
                .with_file(true)
                .with_line_number(true);

            tracing_subscriber::registry()
                .with(filter)
                .with(fmt_layer)
                .init();
        }
        _ => {
            let fmt_layer = fmt::layer()
                .with_writer(std::io::stdout)
                .with_target(true)
                .with_thread_ids(false)
                .with_file(false);

            tracing_subscriber::registry()
                .with(filter)
                .with(fmt_layer)
                .init();
        }
    }

    if let Some(ref access_log) = config.access_log {
        tracing::info!("Access log will be written to: {}", access_log);
    }

    Ok(())
}
