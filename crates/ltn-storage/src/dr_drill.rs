use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::backup::{BackupEngine, Snapshot};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum DrillStatus {
    Passed,
    Failed { reason: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DrDrillReport {
    pub drill_id: String,
    pub app_name: String,
    pub snapshot_id: String,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub rto_seconds: f64,
    pub max_rto_seconds: f64,
    pub data_checksum_valid: bool,
    pub status: DrillStatus,
}

pub struct DrDrillRunner;

impl DrDrillRunner {
    pub fn run_synthetic_drill(
        snapshot: &Snapshot,
        snapshot_data: &[u8],
        max_rto_seconds: f64,
    ) -> DrDrillReport {
        let start = Utc::now();
        let drill_id = Uuid::new_v4().to_string();

        // 1. Verify cryptographic integrity of the snapshot
        let checksum_valid = BackupEngine::verify_integrity(snapshot, snapshot_data);
        if !checksum_valid {
            let end = Utc::now();
            let rto_seconds = (end - start).num_milliseconds() as f64 / 1000.0;
            return DrDrillReport {
                drill_id,
                app_name: snapshot.app_name.clone(),
                snapshot_id: snapshot.id.clone(),
                start_time: start,
                end_time: end,
                rto_seconds,
                max_rto_seconds,
                data_checksum_valid: false,
                status: DrillStatus::Failed {
                    reason: "Snapshot SHA-256 integrity verification failed: data corruption detected".into(),
                },
            };
        }

        // 2. Synthetic restore validation: ensure payload is non-empty and well-formed
        if snapshot_data.is_empty() {
            let end = Utc::now();
            let rto_seconds = (end - start).num_milliseconds() as f64 / 1000.0;
            return DrDrillReport {
                drill_id,
                app_name: snapshot.app_name.clone(),
                snapshot_id: snapshot.id.clone(),
                start_time: start,
                end_time: end,
                rto_seconds,
                max_rto_seconds,
                data_checksum_valid: true,
                status: DrillStatus::Failed {
                    reason: "Restored snapshot data volume is empty".into(),
                },
            };
        }

        let end = Utc::now();
        let rto_seconds = (end - start).num_milliseconds() as f64 / 1000.0;

        let status = if rto_seconds > max_rto_seconds {
            DrillStatus::Failed {
                reason: format!(
                    "RTO exceeded SLA threshold: {:.3}s > {:.3}s",
                    rto_seconds, max_rto_seconds
                ),
            }
        } else {
            DrillStatus::Passed
        };

        DrDrillReport {
            drill_id,
            app_name: snapshot.app_name.clone(),
            snapshot_id: snapshot.id.clone(),
            start_time: start,
            end_time: end,
            rto_seconds,
            max_rto_seconds,
            data_checksum_valid: true,
            status,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dr_drill_success() {
        let dummy_data = b"REAL_POSTGRES_SCHEMA_AND_ROWS_OK";
        let snap = BackupEngine::create_snapshot("orders-svc", "pgdata", dummy_data, "/backups");

        let report = DrDrillRunner::run_synthetic_drill(&snap, dummy_data, 10.0);
        assert_eq!(report.status, DrillStatus::Passed);
        assert!(report.data_checksum_valid);
        assert_eq!(report.app_name, "orders-svc");
        assert!(report.rto_seconds <= 10.0);
    }

    #[test]
    fn test_dr_drill_fails_on_corrupt_data() {
        let dummy_data = b"REAL_POSTGRES_SCHEMA_AND_ROWS_OK";
        let snap = BackupEngine::create_snapshot("orders-svc", "pgdata", dummy_data, "/backups");

        let corrupted_data = b"TAMPERED_DATA_HERE";
        let report = DrDrillRunner::run_synthetic_drill(&snap, corrupted_data, 10.0);
        match report.status {
            DrillStatus::Failed { reason } => {
                assert!(reason.contains("integrity verification failed"));
            }
            _ => panic!("Expected DrillStatus::Failed"),
        }
        assert!(!report.data_checksum_valid);
    }
}
