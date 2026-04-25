use crate::config::LoggingConfig;
use tracing_subscriber::{fmt, EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

pub fn init_logging(config: &LoggingConfig) -> anyhow::Result<()> {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(&config.level));

    let format = config.format.as_deref().unwrap_or("text");

    match format {
        "json" => {
            // JSON format for machine parsing
            let fmt_layer = fmt::layer()
                .json()
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
            // Default text format (CLF-like)
            let fmt_layer = fmt::layer()
                .with_target(true)
                .with_thread_ids(false)
                .with_file(false);

            tracing_subscriber::registry()
                .with(filter)
                .with(fmt_layer)
                .init();
        }
    }

    // If access_log path is configured, log a note about it
    // (File appender support can be added with tracing-appender)
    if let Some(ref access_log) = config.access_log {
        tracing::info!("Access log will be written to: {}", access_log);
    }

    Ok(())
}
