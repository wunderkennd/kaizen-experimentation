//! Neural contextual bandit policy using a 2-layer MLP.
//!
//! Architecture: Linear(d_in, d_hidden) -> ReLU -> Dropout(p) -> Linear(d_hidden, n_arms)
//!
//! Behind `#[cfg(feature = "gpu")]` — requires libtorch.
//! Thompson-style exploration via Gaussian noise on output logits.

use crate::policy::Policy;
use crate::ArmSelection;
use experimentation_core::error::assert_finite;
use std::collections::HashMap;
use tch::{nn, nn::Module, nn::OptimizerConfig, Device, Kind, Tensor};

/// Configuration for the neural contextual bandit.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NeuralConfig {
    pub d_hidden: i64,
    pub dropout_p: f64,
    pub learning_rate: f64,
    /// Scale of Thompson-style exploration noise, decays as 1/sqrt(total_rewards + 1).
    pub noise_scale: f64,
}

impl Default for NeuralConfig {
    fn default() -> Self {
        Self {
            d_hidden: 64,
            dropout_p: 0.1,
            learning_rate: 1e-3,
            noise_scale: 1.0,
        }
    }
}

/// Serializable metadata for neural policy state (excludes VarStore weights).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct NeuralMetadata {
    arm_ids: Vec<String>,
    feature_keys: Vec<String>,
    config: NeuralConfig,
    total_rewards: u64,
}

/// Neural contextual bandit policy.
///
/// Uses a 2-layer MLP to predict expected reward per arm given context features.
/// Exploration via Gaussian noise on logits, decaying with 1/sqrt(total_rewards + 1).
pub struct NeuralContextualPolicy {
    vs: nn::VarStore,
    net: nn::Sequential,
    optimizer: nn::Optimizer,
    arm_ids: Vec<String>,
    feature_keys: Vec<String>,
    config: NeuralConfig,
    total_rewards: u64,
}

impl std::fmt::Debug for NeuralContextualPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NeuralContextualPolicy")
            .field("arm_ids", &self.arm_ids)
            .field("feature_keys", &self.feature_keys)
            .field("config", &self.config)
            .field("total_rewards", &self.total_rewards)
            .finish()
    }
}

impl Clone for NeuralContextualPolicy {
    fn clone(&self) -> Self {
        // Serialize/deserialize roundtrip since VarStore doesn't implement Clone.
        let data = self.serialize();
        Self::deserialize(&data)
    }
}

impl NeuralContextualPolicy {
    /// Create a new neural contextual bandit policy.
    ///
    /// # Arguments
    /// - `arm_ids`: Identifiers for each arm (output dimension = arm_ids.len()).
    /// - `feature_keys`: Names of context features (input dimension = feature_keys.len()).
    /// - `config`: Network configuration.
    pub fn new(arm_ids: Vec<String>, feature_keys: Vec<String>, config: NeuralConfig) -> Self {
        assert!(!arm_ids.is_empty(), "must have at least one arm");
        assert!(!feature_keys.is_empty(), "must have at least one feature");

        let d_in = feature_keys.len() as i64;
        let n_arms = arm_ids.len() as i64;
        let vs = nn::VarStore::new(Device::Cpu);
        let net = build_net(&vs.root(), d_in, config.d_hidden, n_arms, config.dropout_p);
        let optimizer = nn::Adam::default()
            .build(&vs, config.learning_rate)
            .expect("optimizer creation should not fail");

        Self {
            vs,
            net,
            optimizer,
            arm_ids,
            feature_keys,
            config,
            total_rewards: 0,
        }
    }

    /// Build input tensor from context features.
    fn context_to_tensor(&self, context: Option<&HashMap<String, f64>>) -> Tensor {
        let values: Vec<f64> = self
            .feature_keys
            .iter()
            .map(|key| {
                let v = context.and_then(|c| c.get(key)).copied().unwrap_or(0.0);
                assert_finite(v, &format!("neural context feature '{key}'"));
                v
            })
            .collect();
        Tensor::from_slice(&values)
            .to_kind(Kind::Float)
            .unsqueeze(0) // batch dim
    }
}

fn build_net(
    root: &nn::Path,
    d_in: i64,
    d_hidden: i64,
    n_arms: i64,
    dropout_p: f64,
) -> nn::Sequential {
    nn::seq()
        .add(nn::linear(
            root / "layer1",
            d_in,
            d_hidden,
            Default::default(),
        ))
        .add_fn(|x| x.relu())
        .add_fn(move |x| x.dropout(dropout_p, false)) // dropout only during training
        .add(nn::linear(
            root / "layer2",
            d_hidden,
            n_arms,
            Default::default(),
        ))
}

impl Policy for NeuralContextualPolicy {
    fn select_arm(&self, context: Option<&HashMap<String, f64>>) -> ArmSelection {
        let input = self.context_to_tensor(context);

        // Forward pass (no_grad, eval mode).
        let logits = tch::no_grad(|| self.net.forward(&input)).squeeze_dim(0);

        // Add Thompson-style exploration noise: scale / sqrt(total_rewards + 1).
        let noise_std = self.config.noise_scale / ((self.total_rewards as f64 + 1.0).sqrt());
        let noise =
            Tensor::randn(self.arm_ids.len() as i64, (Kind::Float, Device::Cpu)) * noise_std;
        let noisy_logits = &logits + &noise;

        // Softmax for probabilities.
        let probs = noisy_logits.softmax(0, Kind::Float);
        let probs_vec: Vec<f64> = Vec::<f64>::try_from(&probs).expect("tensor to vec");

        // Argmax for selection.
        let best_idx = probs_vec
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .unwrap()
            .0;

        let all_arm_probabilities: HashMap<String, f64> = self
            .arm_ids
            .iter()
            .zip(probs_vec.iter())
            .map(|(id, &p)| (id.clone(), p as f64))
            .collect();

        let assignment_probability = all_arm_probabilities[&self.arm_ids[best_idx]];

        ArmSelection {
            arm_id: self.arm_ids[best_idx].clone(),
            assignment_probability,
            all_arm_probabilities,
        }
    }

    fn update(&mut self, arm_id: &str, reward: f64, context: Option<&HashMap<String, f64>>) {
        assert_finite(reward, "neural bandit reward");

        let arm_idx = self
            .arm_ids
            .iter()
            .position(|id| id == arm_id)
            .unwrap_or_else(|| panic!("unknown arm_id: {arm_id}"));

        let input = self.context_to_tensor(context);

        // Forward pass (train mode for dropout).
        let logits = self.net.forward(&input).squeeze_dim(0);

        // MSE loss on the selected arm's output vs reward.
        let target = Tensor::from_slice(&[reward as f32]).to_kind(Kind::Float);
        let prediction = logits.get(arm_idx as i64).unsqueeze(0);
        let loss = prediction.mse_loss(&target, tch::Reduction::Mean);

        // Backward + step.
        self.optimizer.backward_step(&loss);
        self.total_rewards += 1;
    }

    fn serialize(&self) -> Vec<u8> {
        let metadata = NeuralMetadata {
            arm_ids: self.arm_ids.clone(),
            feature_keys: self.feature_keys.clone(),
            config: self.config.clone(),
            total_rewards: self.total_rewards,
        };

        let meta_bytes = serde_json::to_vec(&metadata).expect("metadata serialization");

        // Save VarStore weights to a buffer.
        let mut weights_buf = Vec::new();
        self.vs
            .save_to_stream(&mut weights_buf)
            .expect("VarStore save");

        // Format: [4 bytes meta_len (big-endian)] [meta_bytes] [weights_bytes]
        let meta_len = (meta_bytes.len() as u32).to_be_bytes();
        let mut result = Vec::with_capacity(4 + meta_bytes.len() + weights_buf.len());
        result.extend_from_slice(&meta_len);
        result.extend_from_slice(&meta_bytes);
        result.extend_from_slice(&weights_buf);
        result
    }

    fn deserialize(data: &[u8]) -> Self {
        assert!(data.len() >= 4, "neural policy data too short");

        let meta_len = u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as usize;
        assert!(data.len() >= 4 + meta_len, "neural policy data truncated");

        let metadata: NeuralMetadata =
            serde_json::from_slice(&data[4..4 + meta_len]).expect("metadata deserialization");

        let d_in = metadata.feature_keys.len() as i64;
        let n_arms = metadata.arm_ids.len() as i64;

        let mut vs = nn::VarStore::new(Device::Cpu);
        let net = build_net(
            &vs.root(),
            d_in,
            metadata.config.d_hidden,
            n_arms,
            metadata.config.dropout_p,
        );

        // Load weights from buffer.
        let weights_data = &data[4 + meta_len..];
        let mut cursor = std::io::Cursor::new(weights_data);
        vs.load_from_stream(&mut cursor).expect("VarStore load");

        let optimizer = nn::Adam::default()
            .build(&vs, metadata.config.learning_rate)
            .expect("optimizer creation");

        Self {
            vs,
            net,
            optimizer,
            arm_ids: metadata.arm_ids,
            feature_keys: metadata.feature_keys,
            config: metadata.config,
            total_rewards: metadata.total_rewards,
        }
    }

    fn total_rewards(&self) -> u64 {
        self.total_rewards
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_context(features: &[(&str, f64)]) -> HashMap<String, f64> {
        features.iter().map(|&(k, v)| (k.to_string(), v)).collect()
    }

    #[test]
    fn test_convergence_to_better_arm() {
        let arm_ids = vec!["arm0".into(), "arm1".into()];
        let feature_keys = vec!["f1".into(), "f2".into(), "f3".into()];
        let config = NeuralConfig {
            d_hidden: 32,
            dropout_p: 0.0,
            learning_rate: 0.01,
            noise_scale: 0.5,
        };

        let mut policy = NeuralContextualPolicy::new(arm_ids, feature_keys, config);
        let ctx = make_context(&[("f1", 1.0), ("f2", 0.5), ("f3", 0.0)]);

        // Arm 0 always gives reward 1.0, arm 1 always gives 0.0.
        for _ in 0..500 {
            policy.update("arm0", 1.0, Some(&ctx));
            policy.update("arm1", 0.0, Some(&ctx));
        }

        // After training, arm0 should be selected most of the time.
        let mut arm0_count = 0;
        for _ in 0..100 {
            let selection = policy.select_arm(Some(&ctx));
            if selection.arm_id == "arm0" {
                arm0_count += 1;
            }
        }

        assert!(
            arm0_count > 70,
            "arm0 selected {arm0_count}/100 times, expected >70"
        );
    }

    #[test]
    fn test_serialize_deserialize_roundtrip() {
        let arm_ids = vec!["a".into(), "b".into()];
        let feature_keys = vec!["x".into(), "y".into()];
        let config = NeuralConfig::default();

        let mut policy = NeuralContextualPolicy::new(arm_ids, feature_keys, config);
        let ctx = make_context(&[("x", 1.0), ("y", 2.0)]);
        policy.update("a", 1.0, Some(&ctx));

        let data = policy.serialize();
        let restored = NeuralContextualPolicy::deserialize(&data);

        assert_eq!(restored.total_rewards(), 1);
        assert_eq!(restored.arm_ids, vec!["a", "b"]);
        assert_eq!(restored.feature_keys, vec!["x", "y"]);

        // Both should produce selections (not crash).
        let _sel = restored.select_arm(Some(&ctx));
    }

    #[test]
    #[should_panic(expected = "FAIL-FAST")]
    fn test_nan_context_panics() {
        let policy = NeuralContextualPolicy::new(
            vec!["a".into()],
            vec!["x".into()],
            NeuralConfig::default(),
        );
        let ctx = make_context(&[("x", f64::NAN)]);
        let _sel = policy.select_arm(Some(&ctx));
    }

    #[test]
    fn test_missing_context_uses_zero() {
        let policy = NeuralContextualPolicy::new(
            vec!["a".into(), "b".into()],
            vec!["x".into(), "y".into()],
            NeuralConfig::default(),
        );
        // No context at all.
        let sel = policy.select_arm(None);
        assert!(sel.arm_id == "a" || sel.arm_id == "b");
    }
}
