use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SnapshotStatus {
    Pending,
    Completed,
    Corrupted { reason: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub id: String,
    pub app_name: String,
    pub volume_name: String,
    pub timestamp: DateTime<Utc>,
    pub size_bytes: usize,
    pub sha256_hash: String,
    pub location: String,
    pub status: SnapshotStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetentionPolicy {
    pub max_snapshots: usize,
    pub max_age_hours: u32,
}

impl Default for RetentionPolicy {
    fn default() -> Self {
        Self {
            max_snapshots: 10,
            max_age_hours: 168, // 7 days
        }
    }
}

pub struct BackupEngine;

impl BackupEngine {
    pub fn compute_sha256(data: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(data);
        format!("{:x}", hasher.finalize())
    }

    pub fn create_snapshot(
        app_name: &str,
        volume_name: &str,
        data: &[u8],
        target_dir: &str,
    ) -> Snapshot {
        let id = Uuid::new_v4().to_string();
        let hash = Self::compute_sha256(data);
        let filename = format!("{}_{}_{}.snap", app_name, volume_name, &id[..8]);
        let location = format!("{}/{}", target_dir.trim_end_matches('/'), filename);

        Snapshot {
            id,
            app_name: app_name.to_string(),
            volume_name: volume_name.to_string(),
            timestamp: Utc::now(),
            size_bytes: data.len(),
            sha256_hash: hash,
            location,
            status: SnapshotStatus::Completed,
        }
    }

    pub fn verify_integrity(snapshot: &Snapshot, data: &[u8]) -> bool {
        if snapshot.size_bytes != data.len() {
            return false;
        }
        let current_hash = Self::compute_sha256(data);
        current_hash == snapshot.sha256_hash
    }

    pub fn prune(snapshots: &mut Vec<Snapshot>, policy: &RetentionPolicy) -> Vec<String> {
        let now = Utc::now();
        let mut pruned_ids = Vec::new();

        // 1. Prune by age
        snapshots.retain(|s| {
            let age_hours = (now - s.timestamp).num_hours();
            if age_hours > policy.max_age_hours as i64 {
                pruned_ids.push(s.id.clone());
                false
            } else {
                true
            }
        });

        // 2. Prune by count if exceeds max_snapshots (oldest first)
        if snapshots.len() > policy.max_snapshots {
            snapshots.sort_by_key(|s| s.timestamp);
            let excess = snapshots.len() - policy.max_snapshots;
            let removed: Vec<Snapshot> = snapshots.drain(0..excess).collect();
            for r in removed {
                pruned_ids.push(r.id);
            }
        }

        pruned_ids
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_and_verify_snapshot() {
        let dummy_db_bytes = b"SQLITE_WAL_HEADER_AND_PAGES_DATA_BLOCK";
        let snap = BackupEngine::create_snapshot("prod-api", "postgres-data", dummy_db_bytes, "/var/backups");

        assert_eq!(snap.app_name, "prod-api");
        assert_eq!(snap.volume_name, "postgres-data");
        assert_eq!(snap.size_bytes, dummy_db_bytes.len());
        assert_eq!(snap.status, SnapshotStatus::Completed);
        assert!(!snap.sha256_hash.is_empty());

        // Verify valid data
        assert!(BackupEngine::verify_integrity(&snap, dummy_db_bytes));

        // Verify corrupted data fails
        let corrupted_bytes = b"SQLITE_WAL_HEADER_CORRUPTED_BYTES!!!!!";
        assert!(!BackupEngine::verify_integrity(&snap, corrupted_bytes));
    }

    #[test]
    fn test_retention_prune_by_count() {
        let policy = RetentionPolicy {
            max_snapshots: 2,
            max_age_hours: 1000,
        };

        let mut snaps = vec![
            Snapshot {
                id: "snap-1".into(),
                app_name: "app".into(),
                volume_name: "vol".into(),
                timestamp: Utc::now() - chrono::Duration::hours(10),
                size_bytes: 100,
                sha256_hash: "h1".into(),
                location: "/loc/1".into(),
                status: SnapshotStatus::Completed,
            },
            Snapshot {
                id: "snap-2".into(),
                app_name: "app".into(),
                volume_name: "vol".into(),
                timestamp: Utc::now() - chrono::Duration::hours(5),
                size_bytes: 100,
                sha256_hash: "h2".into(),
                location: "/loc/2".into(),
                status: SnapshotStatus::Completed,
            },
            Snapshot {
                id: "snap-3".into(),
                app_name: "app".into(),
                volume_name: "vol".into(),
                timestamp: Utc::now(),
                size_bytes: 100,
                sha256_hash: "h3".into(),
                location: "/loc/3".into(),
                status: SnapshotStatus::Completed,
            },
        ];

        let pruned = BackupEngine::prune(&mut snaps, &policy);
        assert_eq!(pruned.len(), 1);
        assert_eq!(pruned[0], "snap-1");
        assert_eq!(snaps.len(), 2);
    }
}
