use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricSample {
    pub timestamp_ms: u64,
    pub container_name: String,
    pub cpu_pct: f64,
    pub memory_used_mb: u32,
    pub active_connections: u32,
    pub p99_latency_ms: f64,
}

pub struct DeterministicRingTelemetry {
    capacity: usize,
    samples: Vec<MetricSample>,
    cursor: usize,
}

impl DeterministicRingTelemetry {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            samples: Vec::with_capacity(capacity.max(1)),
            cursor: 0,
        }
    }

    /// Records a new empirical sample without heap re-allocation
    pub fn record(&mut self, sample: MetricSample) {
        if self.samples.len() < self.capacity {
            self.samples.push(sample);
        } else {
            self.samples[self.cursor] = sample;
            self.cursor = (self.cursor + 1) % self.capacity;
        }
    }

    /// Returns average CPU utilization across all recorded samples
    pub fn average_cpu_pct(&self) -> f64 {
        if self.samples.is_empty() {
            return 0.0;
        }
        let sum: f64 = self.samples.iter().map(|s| s.cpu_pct).sum();
        sum / self.samples.len() as f64
    }

    /// Returns average p99 latency across all recorded samples
    pub fn average_p99_latency_ms(&self) -> f64 {
        if self.samples.is_empty() {
            return 0.0;
        }
        let sum: f64 = self.samples.iter().map(|s| s.p99_latency_ms).sum();
        sum / self.samples.len() as f64
    }

    /// Returns total samples currently held in the ring buffer
    pub fn count(&self) -> usize {
        self.samples.len()
    }
}
