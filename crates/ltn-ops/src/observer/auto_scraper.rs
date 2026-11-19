use crate::observer::tsdb_ring::{RollingPercentiles, TsdbCircularRing};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScrapedTargetReport {
    pub target_url: String,
    pub rolling_percentiles: RollingPercentiles,
    pub status: String,
    pub auto_healed: bool,
}

pub struct AutoMetricsScraper;

impl AutoMetricsScraper {
    /// Autonomously scrapes target service endpoint and updates TSDB ring buffer
    pub fn scrape_and_analyze(target_url: &str, ring: &TsdbCircularRing) -> ScrapedTargetReport {
        let t0 = std::time::Instant::now();
        let http_res = ltn_providers::actuators::http_client::MinimalHttpClient::request(
            "GET",
            target_url,
            &[("User-Agent", "ltn-auto-prometheus/1.0")],
            None,
        );
        let latency_ms = (t0.elapsed().as_secs_f64() * 1000.0 * 10.0).round() / 10.0;
        let is_ok = match http_res {
            Ok(resp) => resp.status_code >= 200 && resp.status_code < 400,
            Err(_) => false,
        };

        ring.ingest_probe(target_url, latency_ms, is_ok);
        let percentiles = ring.compute_percentiles();

        let status = if percentiles.error_rate_percent > 1.0 || percentiles.p99_ms > 200.0 {
            "DEGRADED_DRIFT"
        } else {
            "OPTIMAL"
        };

        ScrapedTargetReport {
            target_url: target_url.to_string(),
            rolling_percentiles: percentiles,
            status: status.to_string(),
            auto_healed: false,
        }
    }

    /// Autonomously exports enterprise telemetry in Prometheus exposition or OTel JSON format
    pub fn export_enterprise_telemetry(
        target_url: &str,
        ring: &TsdbCircularRing,
        format_type: &str,
    ) -> String {
        match format_type.to_lowercase().as_str() {
            "prometheus" | "prom" => ring.to_prometheus_exposition(target_url),
            "otel" | "otlp" => {
                serde_json::to_string_pretty(&ring.to_opentelemetry_otlp_json(target_url))
                    .unwrap_or_else(|_| "{}".to_string())
            }
            _ => {
                let report = Self::scrape_and_analyze(target_url, ring);
                serde_json::to_string_pretty(&report).unwrap_or_default()
            }
        }
    }
}
