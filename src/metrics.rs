use prometheus_client::encoding::text::encode;
use prometheus_client::encoding::{EncodeLabelSet, EncodeLabelValue};
use prometheus_client::metrics::counter::Counter;
use prometheus_client::metrics::family::Family;
use prometheus_client::metrics::gauge::Gauge;
use prometheus_client::metrics::histogram::Histogram;
use prometheus_client::registry::Registry;
use std::fmt;
use std::sync::atomic::AtomicI64;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
pub struct RequestLabels {
    pub protocol: ProtocolLabel,
    pub action: ActionLabel,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelValue)]
pub enum ProtocolLabel {
    #[allow(non_camel_case_types)]
    http,
    #[allow(non_camel_case_types)]
    socks5,
}

impl fmt::Display for ProtocolLabel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProtocolLabel::http => write!(f, "http"),
            ProtocolLabel::socks5 => write!(f, "socks5"),
        }
    }
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelValue)]
pub enum ActionLabel {
    #[allow(non_camel_case_types)]
    forward,
    #[allow(non_camel_case_types)]
    connect,
    #[allow(non_camel_case_types)]
    tcp_connect,
    #[allow(non_camel_case_types)]
    udp_associate,
}

impl fmt::Display for ActionLabel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ActionLabel::forward => write!(f, "forward"),
            ActionLabel::connect => write!(f, "connect"),
            ActionLabel::tcp_connect => write!(f, "tcp_connect"),
            ActionLabel::udp_associate => write!(f, "udp_associate"),
        }
    }
}

pub struct ProxyMetrics {
    pub connections_active: Gauge<i64, AtomicI64>,
    pub requests_total: Family<RequestLabels, Counter>,
    pub request_duration_seconds: Histogram,
    pub upstream_errors_total: Counter,
    registry: Registry,
}

impl ProxyMetrics {
    pub fn new() -> Self {
        let mut registry = Registry::default();

        let connections_active = Gauge::<i64, AtomicI64>::default();
        registry.register(
            "peregrine_connections_active",
            "Currently active connections",
            connections_active.clone(),
        );

        let requests_total = Family::<RequestLabels, Counter>::default();
        registry.register(
            "peregrine_requests_total",
            "Total requests handled",
            requests_total.clone(),
        );

        let request_duration_seconds =
            Histogram::new([0.001, 0.005, 0.01, 0.05, 0.1, 0.25, 0.5, 1.0, 5.0, 10.0].into_iter());
        registry.register(
            "peregrine_request_duration_seconds",
            "Request duration in seconds",
            request_duration_seconds.clone(),
        );

        let upstream_errors_total = Counter::default();
        registry.register(
            "peregrine_upstream_errors_total",
            "Total upstream connection errors",
            upstream_errors_total.clone(),
        );

        Self {
            connections_active,
            requests_total,
            request_duration_seconds,
            upstream_errors_total,
            registry,
        }
    }

    pub fn encode(&self) -> String {
        let mut buf = String::new();
        encode(&mut buf, &self.registry).unwrap();
        buf
    }

    pub async fn start_server(
        metrics: Arc<Self>,
        listen_addr: String,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let listener = match TcpListener::bind(&listen_addr).await {
                Ok(listener) => listener,
                Err(e) => {
                    tracing::error!("Failed to bind metrics server to {}: {}", listen_addr, e);
                    return;
                }
            };

            tracing::info!("Metrics server listening on {}", listen_addr);

            loop {
                match listener.accept().await {
                    Ok((mut socket, _)) => {
                        let metrics_clone = Arc::clone(&metrics);
                        tokio::spawn(async move {
                            let mut buffer = [0; 1024];
                            match socket.read(&mut buffer).await {
                                Ok(n) => {
                                    let request = String::from_utf8_lossy(&buffer[..n]);
                                    let response = if request.starts_with("GET /metrics") {
                                        let metrics_text = metrics_clone.encode();
                                        format!(
                                            "HTTP/1.1 200 OK\r\nContent-Type: text/plain; version=0.0.4; charset=utf-8\r\nContent-Length: {}\r\n\r\n{}",
                                            metrics_text.len(),
                                            metrics_text
                                        )
                                    } else if request.starts_with("GET /health") {
                                        "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 2\r\n\r\nOK".to_string()
                                    } else {
                                        "HTTP/1.1 404 Not Found\r\nContent-Type: text/plain\r\nContent-Length: 9\r\n\r\nNot Found".to_string()
                                    };

                                    let _ = socket.write_all(response.as_bytes()).await;
                                }
                                Err(e) => {
                                    tracing::debug!(
                                        "Failed to read from metrics connection: {}",
                                        e
                                    );
                                }
                            }
                        });
                    }
                    Err(e) => {
                        tracing::error!("Failed to accept metrics connection: {}", e);
                    }
                }
            }
        })
    }
}

impl Default for ProxyMetrics {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_creation_and_registration() {
        let metrics = ProxyMetrics::new();

        // Verify metrics can be accessed
        metrics.connections_active.inc();
        assert_eq!(metrics.connections_active.get(), 1);

        metrics.upstream_errors_total.inc();
        assert_eq!(metrics.upstream_errors_total.get(), 1);

        // Test request metrics with labels
        let labels = RequestLabels {
            protocol: ProtocolLabel::http,
            action: ActionLabel::forward,
        };
        metrics.requests_total.get_or_create(&labels).inc();
        assert_eq!(metrics.requests_total.get_or_create(&labels).get(), 1);

        // Test histogram - just check we can observe values
        metrics.request_duration_seconds.observe(0.1);
        // Note: prometheus-client 0.22 doesn't expose sample_count() directly
        // We can verify the histogram works by encoding and checking the output
    }

    #[test]
    fn test_counter_operations() {
        let metrics = ProxyMetrics::new();

        // Test counter increments
        metrics.upstream_errors_total.inc();
        metrics.upstream_errors_total.inc_by(5);
        assert_eq!(metrics.upstream_errors_total.get(), 6);
    }

    #[test]
    fn test_gauge_operations() {
        let metrics = ProxyMetrics::new();

        // Test gauge inc/dec
        metrics.connections_active.inc();
        metrics.connections_active.inc_by(10);
        assert_eq!(metrics.connections_active.get(), 11);

        metrics.connections_active.dec();
        metrics.connections_active.dec_by(5);
        assert_eq!(metrics.connections_active.get(), 5);
    }

    #[test]
    fn test_histogram_operations() {
        let metrics = ProxyMetrics::new();

        // Test histogram observations - verify it doesn't panic
        metrics.request_duration_seconds.observe(0.001);
        metrics.request_duration_seconds.observe(0.1);
        metrics.request_duration_seconds.observe(1.0);

        // Check the encoded output contains histogram info
        let encoded = metrics.encode();
        assert!(encoded.contains("peregrine_request_duration_seconds"));
    }

    #[test]
    fn test_family_metrics_with_different_labels() {
        let metrics = ProxyMetrics::new();

        let http_forward = RequestLabels {
            protocol: ProtocolLabel::http,
            action: ActionLabel::forward,
        };

        let socks5_connect = RequestLabels {
            protocol: ProtocolLabel::socks5,
            action: ActionLabel::tcp_connect,
        };

        metrics
            .requests_total
            .get_or_create(&http_forward)
            .inc_by(5);
        metrics
            .requests_total
            .get_or_create(&socks5_connect)
            .inc_by(3);

        assert_eq!(metrics.requests_total.get_or_create(&http_forward).get(), 5);
        assert_eq!(
            metrics.requests_total.get_or_create(&socks5_connect).get(),
            3
        );
    }

    #[test]
    fn test_metrics_encoding() {
        let metrics = ProxyMetrics::new();

        // Add some test data
        metrics.connections_active.inc_by(42);
        metrics.upstream_errors_total.inc_by(7);

        let labels = RequestLabels {
            protocol: ProtocolLabel::http,
            action: ActionLabel::forward,
        };
        metrics.requests_total.get_or_create(&labels).inc_by(123);

        metrics.request_duration_seconds.observe(0.5);

        let encoded = metrics.encode();

        // Verify the encoded text contains our metrics
        assert!(encoded.contains("peregrine_connections_active"));
        assert!(encoded.contains("peregrine_requests_total"));
        assert!(encoded.contains("peregrine_request_duration_seconds"));
        assert!(encoded.contains("peregrine_upstream_errors_total"));

        // Verify values
        assert!(encoded.contains("42")); // connections_active value
        assert!(encoded.contains("7")); // upstream_errors_total value
        assert!(encoded.contains("123")); // requests_total value

        // Verify labels are present
        assert!(encoded.contains("protocol=\"http\""));
        assert!(encoded.contains("action=\"forward\""));
    }

    #[test]
    fn test_protocol_label_encoding() {
        // Test that our label enums are properly encoded
        assert_eq!(format!("{}", ProtocolLabel::http), "http");
        assert_eq!(format!("{}", ProtocolLabel::socks5), "socks5");
    }

    #[test]
    fn test_action_label_encoding() {
        // Test that our action enums are properly encoded
        assert_eq!(format!("{}", ActionLabel::forward), "forward");
        assert_eq!(format!("{}", ActionLabel::connect), "connect");
        assert_eq!(format!("{}", ActionLabel::tcp_connect), "tcp_connect");
        assert_eq!(format!("{}", ActionLabel::udp_associate), "udp_associate");
    }
}
