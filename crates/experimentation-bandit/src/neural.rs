//! Neural contextual bandit policy using a 2-layer MLP.
//!
//! Architecture: Linear(d_in, d_hidden) -> ReLU -> Dropout(p) -> Linear(d_hidden, n_arms)
//!
//! Behind `#[cfg(feature = "gpu")]` — uses Candle (pure Rust ML framework).
//! Thompson-style exploration via Gaussian noise on output logits.

use crate::policy::Policy;
use crate::ArmSelection;
use candle_core::{DType, Device, IndexOp, ModuleT, Result as CResult, Tensor};
use candle_nn::{
    linear, AdamW, Dropout, Linear, Module, Optimizer, ParamsAdamW, VarBuilder, VarMap,
};
use experimentation_core::error::assert_finite;
use std::collections::HashMap;

/// Configuration for the neural contextual bandit.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NeuralConfig {
    pub d_hidden: usize,
    pub dropout_p: f32,
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

/// Serializable metadata for neural policy state (excludes network weights).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct NeuralMetadata {
    arm_ids: Vec<String>,
    feature_keys: Vec<String>,
    config: NeuralConfig,
    total_rewards: u64,
}

/// 2-layer MLP network.
///
/// Replaces tch's `nn::Sequential` with explicit layer management,
/// giving us proper control over the `train` flag for dropout.
struct Net {
    layer1: Linear,
    layer2: Linear,
    dropout: Dropout,
}

impl Net {
    fn new(
        vb: VarBuilder,
        d_in: usize,
        d_hidden: usize,
        n_arms: usize,
        dropout_p: f32,
    ) -> CResult<Self> {
        let layer1 = linear(d_in, d_hidden, vb.pp("layer1"))?;
        let layer2 = linear(d_hidden, n_arms, vb.pp("layer2"))?;
        let dropout = Dropout::new(dropout_p);
        Ok(Self {
            layer1,
            layer2,
            dropout,
        })
    }

    fn forward(&self, x: &Tensor, train: bool) -> CResult<Tensor> {
        let x = self.layer1.forward(x)?;
        let x = x.relu()?;
        let x = self.dropout.forward_t(&x, train)?;
        self.layer2.forward(&x)
    }
}

/// Neural contextual bandit policy.
///
/// Uses a 2-layer MLP to predict expected reward per arm given context features.
/// Exploration via Gaussian noise on logits, decaying with 1/sqrt(total_rewards + 1).
pub struct NeuralContextualPolicy {
    varmap: VarMap,
    net: Net,
    optimizer: AdamW,
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
        // Serialize/deserialize roundtrip since VarMap doesn't implement Clone.
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

        let d_in = feature_keys.len();
        let n_arms = arm_ids.len();
        let varmap = VarMap::new();
        let vb = VarBuilder::from_varmap(&varmap, DType::F32, &Device::Cpu);
        let net = Net::new(vb, d_in, config.d_hidden, n_arms, config.dropout_p)
            .expect("network construction should not fail");
        let optimizer = AdamW::new(
            varmap.all_vars(),
            ParamsAdamW {
                lr: config.learning_rate,
                ..Default::default()
            },
        )
        .expect("optimizer creation should not fail");

        Self {
            varmap,
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
        let values: Vec<f32> = self
            .feature_keys
            .iter()
            .map(|key| {
                let v = context.and_then(|c| c.get(key)).copied().unwrap_or(0.0);
                assert_finite(v, &format!("neural context feature '{key}'"));
                v as f32
            })
            .collect();
        Tensor::from_slice(&values, (1, values.len()), &Device::Cpu)
            .expect("FAIL-FAST: tensor creation from context")
    }

    /// Inner select_arm returning candle Result for clean `?` propagation.
    fn select_arm_inner(&self, context: Option<&HashMap<String, f64>>) -> CResult<ArmSelection> {
        let input = self.context_to_tensor(context);

        // Forward pass (eval mode: no dropout).
        let logits = self.net.forward(&input, false)?.squeeze(0)?;
        // Fail-fast: verify logits are finite before adding noise.
        let logits_vec: Vec<f32> = logits.to_vec1()?;
        for (i, &v) in logits_vec.iter().enumerate() {
            assert_finite(v as f64, &format!("neural logits[{}]", i));
        }

        // Add Thompson-style exploration noise: scale / sqrt(total_rewards + 1).
        let noise_std = self.config.noise_scale / ((self.total_rewards as f64 + 1.0).sqrt());
        assert_finite(noise_std, "neural noise_std");
        let n_arms = self.arm_ids.len();
        let noise = (Tensor::randn(0f32, 1f32, (n_arms,), &Device::Cpu)? * noise_std)?;
        let noisy_logits = (&logits + &noise)?;
        // Fail-fast: verify noisy logits are finite before softmax.
        let noisy_vec: Vec<f32> = noisy_logits.to_vec1()?;
        for (i, &v) in noisy_vec.iter().enumerate() {
            assert_finite(v as f64, &format!("neural noisy_logits[{}]", i));
        }

        // Softmax for probabilities.
        let probs = candle_nn::ops::softmax(&noisy_logits, 0)?;
        let probs_vec: Vec<f32> = probs.to_vec1()?;

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
            .map(|(id, &p)| {
                let p_f64 = p as f64;
                assert_finite(p_f64, &format!("neural arm probability for '{id}'"));
                (id.clone(), p_f64)
            })
            .collect();

        let assignment_probability = all_arm_probabilities[&self.arm_ids[best_idx]];

        Ok(ArmSelection {
            arm_id: self.arm_ids[best_idx].clone(),
            assignment_probability,
            all_arm_probabilities,
        })
    }

    /// Inner update returning candle Result for clean `?` propagation.
    fn update_inner(
        &mut self,
        arm_id: &str,
        reward: f64,
        context: Option<&HashMap<String, f64>>,
    ) -> CResult<()> {
        let arm_idx = self
            .arm_ids
            .iter()
            .position(|id| id == arm_id)
            .unwrap_or_else(|| panic!("unknown arm_id: {arm_id}"));

        let input = self.context_to_tensor(context);

        // Forward pass (train mode: apply dropout).
        let logits = self.net.forward(&input, true)?.squeeze(0)?;

        // MSE loss on the selected arm's output vs reward.
        let target = Tensor::from_slice(&[reward as f32], (1,), &Device::Cpu)?;
        let prediction = logits.i(arm_idx)?.unsqueeze(0)?;
        let loss = candle_nn::loss::mse(&prediction, &target)?;

        // Backward + step.
        self.optimizer.backward_step(&loss)?;
        self.total_rewards += 1;

        Ok(())
    }
}

impl Policy for NeuralContextualPolicy {
    fn select_arm(&self, context: Option<&HashMap<String, f64>>) -> ArmSelection {
        self.select_arm_inner(context)
            .expect("FAIL-FAST: neural select_arm failed")
    }

    fn update(&mut self, arm_id: &str, reward: f64, context: Option<&HashMap<String, f64>>) {
        assert_finite(reward, "neural bandit reward");
        self.update_inner(arm_id, reward, context)
            .expect("FAIL-FAST: neural update failed")
    }

    fn serialize(&self) -> Vec<u8> {
        let metadata = NeuralMetadata {
            arm_ids: self.arm_ids.clone(),
            feature_keys: self.feature_keys.clone(),
            config: self.config.clone(),
            total_rewards: self.total_rewards,
        };

        let meta_bytes = serde_json::to_vec(&metadata).expect("metadata serialization");

        // Save VarMap weights to a temp file, then read into buffer.
        // VarMap uses safetensors format. tempfile avoids needing the safetensors
        // crate directly for in-memory serialization.
        let tmp = tempfile::NamedTempFile::new().expect("tempfile creation");
        self.varmap.save(tmp.path()).expect("VarMap save");
        let weights_bytes = std::fs::read(tmp.path()).expect("read weights");

        // Format: [4 bytes meta_len (big-endian)] [meta_bytes] [safetensors bytes]
        let meta_len = (meta_bytes.len() as u32).to_be_bytes();
        let mut result = Vec::with_capacity(4 + meta_bytes.len() + weights_bytes.len());
        result.extend_from_slice(&meta_len);
        result.extend_from_slice(&meta_bytes);
        result.extend_from_slice(&weights_bytes);
        result
    }

    fn deserialize(data: &[u8]) -> Self {
        assert!(data.len() >= 4, "neural policy data too short");

        let meta_len = u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as usize;
        assert!(data.len() >= 4 + meta_len, "neural policy data truncated");

        let metadata: NeuralMetadata =
            serde_json::from_slice(&data[4..4 + meta_len]).expect("metadata deserialization");

        let d_in = metadata.feature_keys.len();
        let n_arms = metadata.arm_ids.len();

        // Reconstruct network structure first (creates variables in VarMap),
        // then load saved weights into those variables.
        let mut varmap = VarMap::new();
        let vb = VarBuilder::from_varmap(&varmap, DType::F32, &Device::Cpu);
        let net = Net::new(
            vb,
            d_in,
            metadata.config.d_hidden,
            n_arms,
            metadata.config.dropout_p,
        )
        .expect("network reconstruction");

        // Write weights to temp file, then load into VarMap.
        let weights_data = &data[4 + meta_len..];
        let tmp = tempfile::NamedTempFile::new().expect("tempfile creation");
        std::fs::write(tmp.path(), weights_data).expect("write weights to tempfile");
        varmap.load(tmp.path()).expect("VarMap load");

        let optimizer = AdamW::new(
            varmap.all_vars(),
            ParamsAdamW {
                lr: metadata.config.learning_rate,
                ..Default::default()
            },
        )
        .expect("optimizer creation");

        Self {
            varmap,
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
