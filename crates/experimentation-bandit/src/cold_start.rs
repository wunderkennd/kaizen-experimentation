//! Content cold-start bandit for new content launches.
//!
//! When new content is added to the catalog with no historical data,
//! a cold-start bandit explores recommendation placements to learn
//! which placements work best for which user segments.
//!
//! After the cold-start window closes (default 7 days), the learned
//! affinity scores are exported to the recommendation system.

use crate::linucb::LinUcbPolicy;
use crate::policy::AnyPolicy;
use std::collections::HashMap;

/// Configuration for a cold-start bandit experiment.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ColdStartConfig {
    /// Content being explored.
    pub content_id: String,
    /// Content metadata (genre, language, etc.) used as context features.
    pub content_metadata: HashMap<String, String>,
    /// How many days the cold-start window runs (default 7).
    pub window_days: i32,
    /// Arm IDs representing recommendation placements.
    pub arm_ids: Vec<String>,
    /// Context feature keys for user segment features.
    pub feature_keys: Vec<String>,
    /// Exploration parameter for LinUCB.
    pub alpha: f64,
    /// Minimum exploration fraction per arm.
    pub min_exploration_fraction: f64,
}

impl ColdStartConfig {
    /// Generate a deterministic experiment ID from the content ID.
    pub fn experiment_id(&self) -> String {
        format!("cold-start:{}", self.content_id)
    }

    /// Default window days.
    pub const DEFAULT_WINDOW_DAYS: i32 = 7;
}

/// Result of exporting affinity scores after cold-start window closes.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AffinityScores {
    /// Content ID this export is for.
    pub content_id: String,
    /// Per-segment predicted reward: segment_name → predicted engagement score.
    pub segment_affinity_scores: HashMap<String, f64>,
    /// Per-segment optimal placement: segment_name → arm_id with highest predicted reward.
    pub optimal_placements: HashMap<String, String>,
}

/// Compute affinity scores from a trained LinUCB policy.
///
/// For each user segment (defined by a context feature vector), compute the
/// predicted reward per arm and select the arm with the highest prediction.
///
/// # Arguments
/// * `policy` — Trained LinUCB policy
/// * `content_id` — Content identifier for the export
/// * `segment_contexts` — Map of segment_name → context feature vector
pub fn export_affinity_scores(
    policy: &LinUcbPolicy,
    content_id: &str,
    segment_contexts: &HashMap<String, HashMap<String, f64>>,
) -> AffinityScores {
    let mut segment_affinity_scores = HashMap::new();
    let mut optimal_placements = HashMap::new();

    for (segment_name, context) in segment_contexts {
        let mut best_arm_id = String::new();
        let mut best_reward = f64::NEG_INFINITY;

        for arm in policy.arms() {
            let predicted = policy.predicted_reward(&arm.arm_id, context);
            if predicted > best_reward {
                best_reward = predicted;
                best_arm_id = arm.arm_id.clone();
            }
        }

        segment_affinity_scores.insert(segment_name.clone(), best_reward);
        optimal_placements.insert(segment_name.clone(), best_arm_id);
    }

    AffinityScores {
        content_id: content_id.to_string(),
        segment_affinity_scores,
        optimal_placements,
    }
}

/// Create a cold-start bandit policy from config.
///
/// Returns the experiment_id and the AnyPolicy wrapping a LinUCB policy.
pub fn create_cold_start_policy(config: &ColdStartConfig) -> (String, AnyPolicy) {
    let experiment_id = config.experiment_id();
    let policy = LinUcbPolicy::new(
        experiment_id.clone(),
        config.arm_ids.clone(),
        config.feature_keys.clone(),
        config.alpha,
        config.min_exploration_fraction,
    );
    (experiment_id, AnyPolicy::LinUcb(policy))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> ColdStartConfig {
        ColdStartConfig {
            content_id: "movie-123".into(),
            content_metadata: [("genre".into(), "action".into())].into_iter().collect(),
            window_days: 7,
            arm_ids: vec![
                "homepage_featured".into(),
                "trending_tab".into(),
                "notification".into(),
            ],
            feature_keys: vec!["user_age_bucket".into(), "watch_history_len".into()],
            alpha: 1.0,
            min_exploration_fraction: 0.05,
        }
    }

    fn segment_context(age_bucket: f64, watch_len: f64) -> HashMap<String, f64> {
        [
            ("user_age_bucket".into(), age_bucket),
            ("watch_history_len".into(), watch_len),
        ]
        .into_iter()
        .collect()
    }

    #[test]
    fn test_experiment_id_generation() {
        let config = test_config();
        assert_eq!(config.experiment_id(), "cold-start:movie-123");
    }

    #[test]
    fn test_create_cold_start_policy() {
        let config = test_config();
        let (exp_id, policy) = create_cold_start_policy(&config);

        assert_eq!(exp_id, "cold-start:movie-123");
        assert_eq!(policy.policy_type(), "linucb");

        // Should be able to select an arm with context
        let ctx = segment_context(2.0, 50.0);
        let selection = policy.select_arm(Some(&ctx));
        assert!(config.arm_ids.contains(&selection.arm_id));
    }

    #[test]
    fn test_export_affinity_scores_untrained() {
        let config = test_config();
        let (_, policy) = create_cold_start_policy(&config);

        // Extract LinUCB from AnyPolicy
        let linucb = match &policy {
            AnyPolicy::LinUcb(p) => p,
            _ => panic!("expected LinUcb"),
        };

        let segments: HashMap<String, HashMap<String, f64>> = [
            ("new_users".to_string(), segment_context(1.0, 5.0)),
            ("power_users".to_string(), segment_context(3.0, 200.0)),
        ]
        .into_iter()
        .collect();

        let scores = export_affinity_scores(linucb, "movie-123", &segments);
        assert_eq!(scores.content_id, "movie-123");
        assert_eq!(scores.segment_affinity_scores.len(), 2);
        assert_eq!(scores.optimal_placements.len(), 2);

        // Untrained policy: all θ vectors are zero, so all predicted rewards are 0.0
        for score in scores.segment_affinity_scores.values() {
            assert!(score.is_finite());
        }
    }

    #[test]
    fn test_export_affinity_scores_after_training() {
        let config = test_config();
        let (_, mut policy) = create_cold_start_policy(&config);

        // Train: homepage_featured gets high rewards for power users
        let power_user_ctx = segment_context(3.0, 200.0);
        let new_user_ctx = segment_context(1.0, 5.0);

        for _ in 0..100 {
            policy.update("homepage_featured", 1.0, Some(&power_user_ctx));
            policy.update("trending_tab", 0.2, Some(&power_user_ctx));
            policy.update("notification", 0.1, Some(&power_user_ctx));

            policy.update("notification", 0.8, Some(&new_user_ctx));
            policy.update("homepage_featured", 0.2, Some(&new_user_ctx));
            policy.update("trending_tab", 0.3, Some(&new_user_ctx));
        }

        let linucb = match &policy {
            AnyPolicy::LinUcb(p) => p,
            _ => panic!("expected LinUcb"),
        };

        let segments: HashMap<String, HashMap<String, f64>> = [
            ("new_users".to_string(), new_user_ctx),
            ("power_users".to_string(), power_user_ctx),
        ]
        .into_iter()
        .collect();

        let scores = export_affinity_scores(linucb, "movie-123", &segments);

        // Power users should prefer homepage_featured
        assert_eq!(
            scores.optimal_placements.get("power_users").unwrap(),
            "homepage_featured",
            "power users should prefer homepage_featured"
        );

        // New users should prefer notification
        assert_eq!(
            scores.optimal_placements.get("new_users").unwrap(),
            "notification",
            "new users should prefer notification"
        );

        // Power users should have higher affinity score than new users
        // (homepage_featured reward=1.0 vs notification reward=0.8)
        let power_score = scores.segment_affinity_scores["power_users"];
        let new_score = scores.segment_affinity_scores["new_users"];
        assert!(
            power_score > new_score,
            "power user affinity ({power_score}) should exceed new user ({new_score})"
        );
    }

    #[test]
    fn test_cold_start_serialize_roundtrip() {
        let config = test_config();
        let (_, mut policy) = create_cold_start_policy(&config);

        let ctx = segment_context(2.0, 50.0);
        for _ in 0..20 {
            policy.update("homepage_featured", 1.0, Some(&ctx));
        }

        let data = policy.serialize();
        let restored = AnyPolicy::deserialize("linucb", &data);
        assert_eq!(restored.policy_type(), "linucb");
        assert_eq!(restored.total_rewards(), 20);
    }

    #[test]
    fn test_cold_start_config_serialize() {
        let config = test_config();
        let json = serde_json::to_string(&config).unwrap();
        let restored: ColdStartConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.content_id, "movie-123");
        assert_eq!(restored.window_days, 7);
        assert_eq!(restored.arm_ids.len(), 3);
    }
}
