//! Bandit arm selection client.
//!
//! Provides both a live gRPC client to M4b's `BanditPolicyService.SelectArm`
//! and a mock uniform-random fallback used when M4b is unavailable or times out.
//! Per onboarding pitfall #4: timeout at 10ms, fall back to uniform random.

use std::collections::HashMap;
use std::time::Duration;

use rand::Rng;
use tonic::transport::Channel;

use crate::config::{BanditArmConfig, BanditConfig};

use experimentation_core::error::assert_finite;
use experimentation_proto::experimentation::bandit::v1::{
    bandit_policy_service_client::BanditPolicyServiceClient, CreateColdStartBanditRequest,
    ExportAffinityScoresRequest, SelectArmRequest,
};

/// Default timeout for M4b SelectArm RPC (per onboarding pitfall #4).
const BANDIT_TIMEOUT: Duration = Duration::from_millis(10);

/// Timeout for cold-start management RPCs (not hot-path, can be slower).
const COLD_START_TIMEOUT: Duration = Duration::from_millis(5000);

/// Result of selecting an arm from a bandit experiment.
#[derive(Debug, Clone)]
pub struct ArmSelection {
    /// The selected arm ID.
    pub arm_id: String,
    /// Assignment probability for this arm at selection time (for IPW logging).
    pub assignment_probability: f64,
    /// Payload JSON for the selected arm.
    pub payload_json: String,
    /// All arm probabilities at selection time.
    pub all_arm_probabilities: HashMap<String, f64>,
}

/// gRPC client wrapper for M4b BanditPolicyService.
///
/// Calls `SelectArm` with a 10ms timeout. On timeout or error, the caller
/// falls back to uniform random arm selection.
#[derive(Clone)]
pub struct GrpcBanditClient {
    client: BanditPolicyServiceClient<Channel>,
    timeout: Duration,
}

impl GrpcBanditClient {
    /// Connect to M4b at the given address (e.g., "http://localhost:50054").
    pub async fn connect(addr: &str) -> Result<Self, tonic::transport::Error> {
        Self::connect_with_timeout(addr, BANDIT_TIMEOUT).await
    }

    /// Connect with a custom per-call timeout.
    ///
    /// This is intended for integration tests and non-hot-path callers.
    /// Production callers should use [`connect`](Self::connect) which enforces
    /// the 10ms SLA timeout.
    #[doc(hidden)]
    pub async fn connect_with_timeout(
        addr: &str,
        timeout: Duration,
    ) -> Result<Self, tonic::transport::Error> {
        let channel = Channel::from_shared(addr.to_string())
            .expect("valid URI")
            .connect()
            .await?;
        Ok(Self {
            client: BanditPolicyServiceClient::new(channel),
            timeout,
        })
    }

    /// Select an arm via M4b's SelectArm RPC.
    ///
    /// Returns `Err` on timeout (>10ms) or gRPC failure — caller should fall back
    /// to uniform random.
    pub async fn select_arm(
        &self,
        experiment_id: &str,
        user_id: &str,
        context_features: HashMap<String, f64>,
    ) -> Result<ArmSelectionResult, BanditClientError> {
        let req = SelectArmRequest {
            experiment_id: experiment_id.to_string(),
            user_id: user_id.to_string(),
            context_features,
        };

        let mut client = self.client.clone();
        let resp = tokio::time::timeout(self.timeout, client.select_arm(req))
            .await
            .map_err(|_| BanditClientError::Timeout)?
            .map_err(BanditClientError::Grpc)?;

        let arm = resp.into_inner();
        Ok(ArmSelectionResult {
            arm_id: arm.arm_id,
            assignment_probability: arm.assignment_probability,
            all_arm_probabilities: arm.all_arm_probabilities,
        })
    }

    /// Create a cold-start bandit for new content via M4b.
    ///
    /// Uses a 5s timeout (management operation, not hot-path).
    pub async fn create_cold_start_bandit(
        &self,
        content_id: &str,
        content_metadata: HashMap<String, String>,
        window_days: i32,
    ) -> Result<ColdStartCreated, BanditClientError> {
        let req = CreateColdStartBanditRequest {
            content_id: content_id.to_string(),
            content_metadata,
            window_days,
        };

        let mut client = self.client.clone();
        let resp = tokio::time::timeout(COLD_START_TIMEOUT, client.create_cold_start_bandit(req))
            .await
            .map_err(|_| BanditClientError::Timeout)?
            .map_err(BanditClientError::Grpc)?;

        let inner = resp.into_inner();
        Ok(ColdStartCreated {
            experiment_id: inner.experiment_id,
            content_id: inner.content_id,
        })
    }

    /// Export learned affinity scores after a cold-start window closes.
    ///
    /// Uses a 5s timeout (management operation, not hot-path).
    /// All returned scores are validated with `assert_finite!()`.
    pub async fn export_affinity_scores(
        &self,
        experiment_id: &str,
    ) -> Result<AffinityScoresResult, BanditClientError> {
        let req = ExportAffinityScoresRequest {
            experiment_id: experiment_id.to_string(),
        };

        let mut client = self.client.clone();
        let resp = tokio::time::timeout(COLD_START_TIMEOUT, client.export_affinity_scores(req))
            .await
            .map_err(|_| BanditClientError::Timeout)?
            .map_err(BanditClientError::Grpc)?;

        let inner = resp.into_inner();

        // Validate all scores are finite (fail-fast data integrity).
        for (segment, score) in &inner.segment_affinity_scores {
            assert_finite(*score, &format!("affinity score for segment '{segment}'"));
        }

        Ok(AffinityScoresResult {
            content_id: inner.content_id,
            segment_affinity_scores: inner.segment_affinity_scores,
            optimal_placements: inner.optimal_placements,
        })
    }
}

/// Result of creating a cold-start bandit for new content.
#[derive(Debug, Clone)]
pub struct ColdStartCreated {
    /// The auto-created experiment ID.
    pub experiment_id: String,
    /// The content ID the bandit was created for.
    pub content_id: String,
}

/// Result of exporting affinity scores after a cold-start window closes.
#[derive(Debug, Clone)]
pub struct AffinityScoresResult {
    /// The content ID the scores are for.
    pub content_id: String,
    /// Per-segment affinity scores: which user segments respond best.
    pub segment_affinity_scores: HashMap<String, f64>,
    /// Optimal placement per segment (arm with highest reward).
    pub optimal_placements: HashMap<String, String>,
}

/// Raw result from M4b (no payload — that comes from local config).
#[derive(Debug)]
pub struct ArmSelectionResult {
    pub arm_id: String,
    pub assignment_probability: f64,
    pub all_arm_probabilities: HashMap<String, f64>,
}

/// Errors from the bandit gRPC client.
#[derive(Debug)]
pub enum BanditClientError {
    Timeout,
    Grpc(tonic::Status),
}

impl std::fmt::Display for BanditClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Timeout => write!(f, "M4b SelectArm timed out (>10ms)"),
            Self::Grpc(status) => write!(f, "M4b SelectArm gRPC error: {status}"),
        }
    }
}

/// Look up the payload_json for a given arm_id from local bandit config.
///
/// M4b only selects the arm — payload is stored in the local experiment config.
pub fn lookup_arm_payload(arms: &[BanditArmConfig], arm_id: &str) -> String {
    arms.iter()
        .find(|a| a.arm_id == arm_id)
        .map(|a| a.payload_json.clone())
        .unwrap_or_default()
}

/// Extract context features from user attributes based on bandit config keys.
///
/// For CONTEXTUAL_BANDIT experiments, parses string attribute values to f64.
/// Non-numeric values are silently skipped.
pub fn extract_context_features(
    bandit_config: &BanditConfig,
    attributes: &HashMap<String, String>,
) -> HashMap<String, f64> {
    bandit_config
        .context_feature_keys
        .iter()
        .filter_map(|key| {
            attributes
                .get(key)
                .and_then(|v| v.parse::<f64>().ok())
                .map(|v| (key.clone(), v))
        })
        .collect()
}

/// Select an arm using uniform random (mock for M4b).
///
/// Each arm gets equal probability `1/n`. The deterministic `rng` ensures
/// the same user + experiment always gets the same arm within a session.
pub fn select_arm_uniform<R: Rng>(
    bandit_config: &BanditConfig,
    rng: &mut R,
) -> Option<ArmSelection> {
    let n = bandit_config.arms.len();
    if n == 0 {
        return None;
    }

    let prob = 1.0 / n as f64;
    let idx = rng.gen_range(0..n);
    let arm = &bandit_config.arms[idx];

    let all_probs: HashMap<String, f64> = bandit_config
        .arms
        .iter()
        .map(|a| (a.arm_id.clone(), prob))
        .collect();

    Some(ArmSelection {
        arm_id: arm.arm_id.clone(),
        assignment_probability: prob,
        payload_json: arm.payload_json.clone(),
        all_arm_probabilities: all_probs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{BanditArmConfig, BanditConfig};
    use rand::rngs::StdRng;
    use rand::SeedableRng;

    fn make_bandit_config(n_arms: usize) -> BanditConfig {
        let arms: Vec<BanditArmConfig> = (0..n_arms)
            .map(|i| BanditArmConfig {
                arm_id: format!("arm_{i}"),
                name: format!("Arm {i}"),
                payload_json: format!(r#"{{"arm":{i}}}"#),
            })
            .collect();

        BanditConfig {
            algorithm: "THOMPSON_SAMPLING".to_string(),
            arms,
            reward_metric_id: "clicks".to_string(),
            context_feature_keys: vec![],
            min_exploration_fraction: 0.1,
            warmup_observations: 1000,
            content_id: None,
            cold_start_window_days: None,
        }
    }

    #[test]
    fn test_uniform_selection_deterministic() {
        let config = make_bandit_config(3);
        let mut rng1 = StdRng::seed_from_u64(42);
        let mut rng2 = StdRng::seed_from_u64(42);

        let sel1 = select_arm_uniform(&config, &mut rng1).unwrap();
        let sel2 = select_arm_uniform(&config, &mut rng2).unwrap();

        assert_eq!(sel1.arm_id, sel2.arm_id);
        assert!((sel1.assignment_probability - sel2.assignment_probability).abs() < f64::EPSILON);
    }

    #[test]
    fn test_uniform_probability() {
        let config = make_bandit_config(4);
        let mut rng = StdRng::seed_from_u64(0);

        let sel = select_arm_uniform(&config, &mut rng).unwrap();
        assert!((sel.assignment_probability - 0.25).abs() < f64::EPSILON);
        assert_eq!(sel.all_arm_probabilities.len(), 4);
        for prob in sel.all_arm_probabilities.values() {
            assert!((*prob - 0.25).abs() < f64::EPSILON);
        }
    }

    #[test]
    fn test_uniform_balance() {
        let config = make_bandit_config(3);
        let mut counts = HashMap::new();

        for seed in 0..3000u64 {
            let mut rng = StdRng::seed_from_u64(seed);
            let sel = select_arm_uniform(&config, &mut rng).unwrap();
            *counts.entry(sel.arm_id).or_insert(0u64) += 1;
        }

        // Each arm should get ~1000 ± 100 out of 3000 trials.
        for (arm, count) in &counts {
            let frac = *count as f64 / 3000.0;
            assert!(
                (0.28..=0.39).contains(&frac),
                "arm {arm} fraction {frac:.3} outside [0.28, 0.39]"
            );
        }
    }

    #[test]
    fn test_empty_arms_returns_none() {
        let config = make_bandit_config(0);
        let mut rng = StdRng::seed_from_u64(0);
        assert!(select_arm_uniform(&config, &mut rng).is_none());
    }

    #[test]
    fn test_single_arm() {
        let config = make_bandit_config(1);
        let mut rng = StdRng::seed_from_u64(0);
        let sel = select_arm_uniform(&config, &mut rng).unwrap();
        assert_eq!(sel.arm_id, "arm_0");
        assert!((sel.assignment_probability - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_payload_propagated() {
        let config = make_bandit_config(2);
        let mut rng = StdRng::seed_from_u64(0);
        let sel = select_arm_uniform(&config, &mut rng).unwrap();
        assert!(!sel.payload_json.is_empty());
    }

    #[test]
    fn test_lookup_arm_payload_found() {
        let arms = vec![
            BanditArmConfig {
                arm_id: "a".to_string(),
                name: "A".to_string(),
                payload_json: r#"{"x":1}"#.to_string(),
            },
            BanditArmConfig {
                arm_id: "b".to_string(),
                name: "B".to_string(),
                payload_json: r#"{"x":2}"#.to_string(),
            },
        ];
        assert_eq!(lookup_arm_payload(&arms, "b"), r#"{"x":2}"#);
    }

    #[test]
    fn test_lookup_arm_payload_not_found() {
        let arms = vec![BanditArmConfig {
            arm_id: "a".to_string(),
            name: "A".to_string(),
            payload_json: "{}".to_string(),
        }];
        assert_eq!(lookup_arm_payload(&arms, "missing"), "");
    }

    #[test]
    fn test_extract_context_features() {
        let config = BanditConfig {
            algorithm: "LINEAR_UCB".to_string(),
            arms: vec![],
            reward_metric_id: "clicks".to_string(),
            context_feature_keys: vec![
                "age".to_string(),
                "tenure".to_string(),
                "missing".to_string(),
            ],
            min_exploration_fraction: 0.1,
            warmup_observations: 1000,
            content_id: None,
            cold_start_window_days: None,
        };
        let mut attrs = HashMap::new();
        attrs.insert("age".to_string(), "25.0".to_string());
        attrs.insert("tenure".to_string(), "3.5".to_string());
        attrs.insert("country".to_string(), "US".to_string()); // not in context_feature_keys

        let features = extract_context_features(&config, &attrs);
        assert_eq!(features.len(), 2);
        assert!((features["age"] - 25.0).abs() < f64::EPSILON);
        assert!((features["tenure"] - 3.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_extract_context_features_non_numeric_skipped() {
        let config = BanditConfig {
            algorithm: "LINEAR_UCB".to_string(),
            arms: vec![],
            reward_metric_id: "clicks".to_string(),
            context_feature_keys: vec!["age".to_string(), "country".to_string()],
            min_exploration_fraction: 0.1,
            warmup_observations: 1000,
            content_id: None,
            cold_start_window_days: None,
        };
        let mut attrs = HashMap::new();
        attrs.insert("age".to_string(), "25".to_string());
        attrs.insert("country".to_string(), "US".to_string()); // not parseable as f64

        let features = extract_context_features(&config, &attrs);
        assert_eq!(features.len(), 1);
        assert!((features["age"] - 25.0).abs() < f64::EPSILON);
    }
}
