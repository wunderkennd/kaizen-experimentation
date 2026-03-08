//! RocksDB snapshot store for bandit policy state (ADR-003).
//!
//! Key format: `{experiment_id}\0{timestamp_millis:020}` (null-byte separator, zero-padded for lexicographic order).
//! Value: JSON-encoded `SnapshotEnvelope`.

use chrono::Utc;
use rocksdb::{IteratorMode, Options, DB};
use std::path::Path;

/// Metadata envelope persisted alongside the serialized policy state.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SnapshotEnvelope {
    pub experiment_id: String,
    /// Algorithm type identifier (e.g., "thompson_sampling", "linucb").
    /// Defaults to "thompson_sampling" for backwards compatibility with old snapshots.
    #[serde(default = "default_policy_type")]
    pub policy_type: String,
    /// Opaque policy state bytes (serialized by the Policy trait).
    pub policy_state: Vec<u8>,
    /// Total rewards processed at time of snapshot.
    pub total_rewards_processed: u64,
    /// Last Kafka offset included in this snapshot.
    pub kafka_offset: i64,
    /// Snapshot creation time (epoch milliseconds).
    pub snapshot_at_epoch_ms: i64,
}

fn default_policy_type() -> String {
    "thompson_sampling".to_string()
}

/// Persistent snapshot store backed by RocksDB.
pub struct SnapshotStore {
    db: DB,
}

impl SnapshotStore {
    /// Open or create a RocksDB instance at the given path.
    pub fn open(path: &Path) -> Result<Self, rocksdb::Error> {
        let mut opts = Options::default();
        opts.create_if_missing(true);
        opts.set_compression_type(rocksdb::DBCompressionType::Lz4);
        let db = DB::open(&opts, path)?;
        Ok(Self { db })
    }

    /// Write a snapshot for an experiment.
    pub fn write_snapshot(&self, envelope: &SnapshotEnvelope) -> Result<(), rocksdb::Error> {
        let key = snapshot_key(&envelope.experiment_id, envelope.snapshot_at_epoch_ms);
        let value = serde_json::to_vec(envelope).expect("snapshot serialization should not fail");
        self.db.put(key.as_bytes(), &value)
    }

    /// Load the latest snapshot for a specific experiment.
    pub fn load_latest(
        &self,
        experiment_id: &str,
    ) -> Result<Option<SnapshotEnvelope>, rocksdb::Error> {
        let prefix = format!("{experiment_id}{}", KEY_SEP);
        // Scan keys with this prefix in reverse to find the latest.
        let iter = self.db.iterator(IteratorMode::End);
        for item in iter {
            let (key, value) = item?;
            let key_str = String::from_utf8_lossy(&key);
            if key_str.starts_with(&prefix) {
                let envelope: SnapshotEnvelope = serde_json::from_slice(&value)
                    .expect("snapshot deserialization should not fail");
                return Ok(Some(envelope));
            }
            if key_str.as_ref() < prefix.as_str() {
                break;
            }
        }
        Ok(None)
    }

    /// Load the latest snapshot for every experiment (used during startup restore).
    pub fn load_all_latest(&self) -> Result<Vec<SnapshotEnvelope>, rocksdb::Error> {
        let mut latest: std::collections::HashMap<String, SnapshotEnvelope> =
            std::collections::HashMap::new();

        let iter = self.db.iterator(IteratorMode::Start);
        for item in iter {
            let (_key, value) = item?;
            let envelope: SnapshotEnvelope = serde_json::from_slice(&value)
                .expect("snapshot deserialization should not fail");
            latest.insert(envelope.experiment_id.clone(), envelope);
        }

        Ok(latest.into_values().collect())
    }

    /// Prune old snapshots, keeping only the `keep` most recent per experiment.
    pub fn prune_old_snapshots(
        &self,
        experiment_id: &str,
        keep: usize,
    ) -> Result<usize, rocksdb::Error> {
        let prefix = format!("{experiment_id}{}", KEY_SEP);
        let mut keys: Vec<String> = Vec::new();

        let iter = self
            .db
            .iterator(IteratorMode::From(prefix.as_bytes(), rocksdb::Direction::Forward));
        for item in iter {
            let (key, _value) = item?;
            let key_str = String::from_utf8_lossy(&key).into_owned();
            if !key_str.starts_with(&prefix) {
                break;
            }
            keys.push(key_str);
        }

        if keys.len() <= keep {
            return Ok(0);
        }

        let to_delete = keys.len() - keep;
        for key in &keys[..to_delete] {
            self.db.delete(key.as_bytes())?;
        }

        Ok(to_delete)
    }

    /// Create a snapshot envelope with the current timestamp.
    pub fn make_envelope(
        experiment_id: String,
        policy_type: String,
        policy_state: Vec<u8>,
        total_rewards_processed: u64,
        kafka_offset: i64,
    ) -> SnapshotEnvelope {
        SnapshotEnvelope {
            experiment_id,
            policy_type,
            policy_state,
            total_rewards_processed,
            kafka_offset,
            snapshot_at_epoch_ms: Utc::now().timestamp_millis(),
        }
    }
}

/// Separator used in snapshot keys.  Must not appear in experiment IDs.
const KEY_SEP: char = '\0';

/// Build a lexicographically sortable snapshot key.
///
/// Panics if `experiment_id` contains the key separator (null byte).
fn snapshot_key(experiment_id: &str, timestamp_millis: i64) -> String {
    assert!(
        !experiment_id.contains(KEY_SEP),
        "experiment_id must not contain the null byte separator"
    );
    let clamped: u64 = if timestamp_millis < 0 {
        0
    } else {
        timestamp_millis as u64
    };
    format!("{experiment_id}{KEY_SEP}{clamped:020}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_db_path(name: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("test-snapshot-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        path
    }

    #[test]
    fn test_write_and_load_latest() {
        let path = temp_db_path("write-load");
        let store = SnapshotStore::open(&path).unwrap();

        let env1 = SnapshotEnvelope {
            experiment_id: "exp-1".into(),
            policy_type: "thompson_sampling".into(),
            policy_state: b"state-v1".to_vec(),
            total_rewards_processed: 100,
            kafka_offset: 42,
            snapshot_at_epoch_ms: 1000,
        };
        let env2 = SnapshotEnvelope {
            experiment_id: "exp-1".into(),
            policy_type: "thompson_sampling".into(),
            policy_state: b"state-v2".to_vec(),
            total_rewards_processed: 200,
            kafka_offset: 84,
            snapshot_at_epoch_ms: 2000,
        };

        store.write_snapshot(&env1).unwrap();
        store.write_snapshot(&env2).unwrap();

        let latest = store.load_latest("exp-1").unwrap().unwrap();
        assert_eq!(latest.total_rewards_processed, 200);
        assert_eq!(latest.policy_state, b"state-v2");

        assert!(store.load_latest("exp-999").unwrap().is_none());

        let _ = std::fs::remove_dir_all(&path);
    }

    #[test]
    fn test_load_all_latest() {
        let path = temp_db_path("load-all");
        let store = SnapshotStore::open(&path).unwrap();

        for (exp, ts) in [("exp-1", 1000), ("exp-1", 2000), ("exp-2", 1500)] {
            store
                .write_snapshot(&SnapshotEnvelope {
                    experiment_id: exp.into(),
                    policy_type: "thompson_sampling".into(),
                    policy_state: vec![],
                    total_rewards_processed: ts as u64,
                    kafka_offset: 0,
                    snapshot_at_epoch_ms: ts,
                })
                .unwrap();
        }

        let all = store.load_all_latest().unwrap();
        assert_eq!(all.len(), 2);

        let exp1 = all.iter().find(|e| e.experiment_id == "exp-1").unwrap();
        assert_eq!(exp1.total_rewards_processed, 2000);

        let _ = std::fs::remove_dir_all(&path);
    }

    #[test]
    fn test_prune_old_snapshots() {
        let path = temp_db_path("prune");
        let store = SnapshotStore::open(&path).unwrap();

        for ts in [1000, 2000, 3000, 4000, 5000] {
            store
                .write_snapshot(&SnapshotEnvelope {
                    experiment_id: "exp-1".into(),
                    policy_type: "thompson_sampling".into(),
                    policy_state: vec![],
                    total_rewards_processed: ts as u64,
                    kafka_offset: 0,
                    snapshot_at_epoch_ms: ts,
                })
                .unwrap();
        }

        let deleted = store.prune_old_snapshots("exp-1", 2).unwrap();
        assert_eq!(deleted, 3);

        let mut count = 0;
        let iter = store.db.iterator(IteratorMode::Start);
        for item in iter {
            let _ = item.unwrap();
            count += 1;
        }
        assert_eq!(count, 2);

        let _ = std::fs::remove_dir_all(&path);
    }

    #[test]
    fn test_prefix_ids_do_not_collide() {
        let path = temp_db_path("prefix-collide");
        let store = SnapshotStore::open(&path).unwrap();

        for (exp, ts) in [("exp", 1000i64), ("exp:sub", 2000)] {
            store
                .write_snapshot(&SnapshotEnvelope {
                    experiment_id: exp.into(),
                    policy_type: "thompson_sampling".into(),
                    policy_state: vec![],
                    total_rewards_processed: ts as u64,
                    kafka_offset: 0,
                    snapshot_at_epoch_ms: ts,
                })
                .unwrap();
        }

        let latest_exp = store.load_latest("exp").unwrap().unwrap();
        assert_eq!(latest_exp.experiment_id, "exp");
        assert_eq!(latest_exp.total_rewards_processed, 1000);

        let latest_sub = store.load_latest("exp:sub").unwrap().unwrap();
        assert_eq!(latest_sub.experiment_id, "exp:sub");
        assert_eq!(latest_sub.total_rewards_processed, 2000);

        let _ = std::fs::remove_dir_all(&path);
    }
}
