use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeightedRouteTarget {
    pub name: String,
    pub backend_url: String,
    pub weight: u8, // 0 - 100
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrafficSplitter {
    pub app_name: String,
    pub baseline: WeightedRouteTarget,
    pub canary: Option<WeightedRouteTarget>,
}

impl TrafficSplitter {
    pub fn new(app_name: &str, baseline_url: &str) -> Self {
        Self {
            app_name: app_name.to_string(),
            baseline: WeightedRouteTarget {
                name: "baseline".to_string(),
                backend_url: baseline_url.to_string(),
                weight: 100,
            },
            canary: None,
        }
    }

    pub fn set_canary(&mut self, canary_url: &str, weight: u8) {
        let clamped = weight.min(100);
        self.canary = Some(WeightedRouteTarget {
            name: "canary".to_string(),
            backend_url: canary_url.to_string(),
            weight: clamped,
        });
        self.baseline.weight = 100 - clamped;
    }

    pub fn clear_canary(&mut self) {
        self.canary = None;
        self.baseline.weight = 100;
    }

    pub fn route_for_bucket(&self, bucket_0_to_99: u8) -> &str {
        if let Some(ref canary) = self.canary {
            if canary.weight > 0 && bucket_0_to_99 < canary.weight {
                return &canary.backend_url;
            }
        }
        &self.baseline.backend_url
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_traffic_splitting_buckets() {
        let mut splitter = TrafficSplitter::new("api", "http://127.0.0.1:18000");
        assert_eq!(splitter.route_for_bucket(0), "http://127.0.0.1:18000");

        // 10% canary
        splitter.set_canary("http://127.0.0.1:18001", 10);
        assert_eq!(splitter.baseline.weight, 90);

        // Buckets 0-9 go to canary
        assert_eq!(splitter.route_for_bucket(0), "http://127.0.0.1:18001");
        assert_eq!(splitter.route_for_bucket(9), "http://127.0.0.1:18001");

        // Buckets 10-99 go to baseline
        assert_eq!(splitter.route_for_bucket(10), "http://127.0.0.1:18000");
        assert_eq!(splitter.route_for_bucket(50), "http://127.0.0.1:18000");
        assert_eq!(splitter.route_for_bucket(99), "http://127.0.0.1:18000");

        // clear canary (instant rollback)
        splitter.clear_canary();
        assert_eq!(splitter.route_for_bucket(0), "http://127.0.0.1:18000");
    }
}
