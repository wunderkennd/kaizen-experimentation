//! Switchback (temporal alternation) assignment logic — ADR-022.
//!
//! Three designs are supported:
//!
//! - **SIMPLE_ALTERNATING** (default): all clusters flip together.
//!   Even blocks → control, odd blocks → treatment.
//!
//! - **REGULAR_BALANCED**: clusters are split into two staggered groups using
//!   a deterministic hash of the cluster value. Group A follows SIMPLE_ALTERNATING;
//!   Group B is inverted. At every moment exactly 50% of clusters are in each arm.
//!
//! - **RANDOMIZED**: each (block_index, cluster_value, experiment_id) triple gets
//!   an independent deterministic pseudo-random assignment via MurmurHash3.
//!
//! ## Washout
//!
//! If `washout_period_secs > 0`, the leading edge of every block is a washout
//! window. Callers MUST check `is_in_washout()` before calling `select_variant()`.
//! During washout the caller returns an empty assignment (user excluded).
//!
//! ## Block alignment
//!
//! Blocks are aligned to the Unix epoch:
//! `block_index = floor(current_unix_secs / block_duration_secs)`
//!
//! This makes block boundaries deterministic and globally consistent across
//! all service replicas with no per-experiment clock synchronisation needed.

use crate::config::{SwitchbackConfig, VariantConfig};

/// Minimum block duration enforced during M5 STARTING validation (1 hour).
pub const MIN_BLOCK_DURATION_SECS: i64 = 3600;

/// Minimum planned cycles enforced during M5 STARTING validation.
pub const MIN_PLANNED_CYCLES: i32 = 4;

/// Compute the current block index aligned to the Unix epoch.
///
/// `block_index = floor(current_unix_secs / block_duration_secs)`
///
/// Returns 0 when `block_duration_secs <= 0` (guard against misconfiguration).
pub fn compute_block_index(current_unix_secs: i64, block_duration_secs: i64) -> i64 {
    if block_duration_secs <= 0 {
        return 0;
    }
    current_unix_secs.div_euclid(block_duration_secs)
}

/// Returns `true` if `current_unix_secs` falls within the washout window at
/// the start of the current block.
///
/// The washout window occupies `[block_start, block_start + washout_period_secs)`.
/// Returns `false` when either period is non-positive.
pub fn is_in_washout(
    current_unix_secs: i64,
    block_duration_secs: i64,
    washout_period_secs: i64,
) -> bool {
    if washout_period_secs <= 0 || block_duration_secs <= 0 {
        return false;
    }
    let offset_in_block = current_unix_secs.rem_euclid(block_duration_secs);
    offset_in_block < washout_period_secs
}

/// Select the treatment/control variant for the current block.
///
/// # Arguments
/// * `block_index` – current block number (from [`compute_block_index`])
/// * `design` – `"SIMPLE_ALTERNATING"`, `"REGULAR_BALANCED"`, or `"RANDOMIZED"`
/// * `cluster_value` – value of the cluster attribute for this request (empty = global)
/// * `experiment_id` – used as a seed component for REGULAR_BALANCED and RANDOMIZED
/// * `variants` – slice of experiment variants; must be non-empty
///
/// # Panics
/// Panics if `variants` is empty (unreachable given M5 creation-time validation).
pub fn select_variant<'a>(
    block_index: i64,
    design: &str,
    cluster_value: &str,
    experiment_id: &str,
    variants: &'a [VariantConfig],
) -> &'a VariantConfig {
    assert!(!variants.is_empty(), "experiment must have at least one variant");

    let control_idx = variants
        .iter()
        .position(|v| v.is_control)
        .unwrap_or(0);

    // For a 2-variant experiment the non-control variant is the treatment.
    // For more variants the treatment slot is the first non-control variant.
    let treatment_idx = if control_idx == 0 {
        1.min(variants.len() - 1)
    } else {
        0
    };

    let use_treatment = match design {
        "REGULAR_BALANCED" => regular_balanced(block_index, cluster_value, experiment_id),
        "RANDOMIZED" => randomized(block_index, cluster_value, experiment_id),
        // SIMPLE_ALTERNATING is the default (catches empty string and unknown values).
        _ => simple_alternating(block_index),
    };

    if use_treatment {
        &variants[treatment_idx]
    } else {
        &variants[control_idx]
    }
}

/// SIMPLE_ALTERNATING: even blocks = control, odd blocks = treatment.
///
/// All clusters move together; no cluster differentiation.
fn simple_alternating(block_index: i64) -> bool {
    block_index % 2 != 0
}

/// REGULAR_BALANCED: clusters split into two staggered groups.
///
/// Cluster group is derived from `murmurhash3(cluster_value, experiment_id) % 2`.
/// Group 0 follows SIMPLE_ALTERNATING; Group 1 is its inverse.
/// At every moment exactly 50% of clusters are in control and 50% in treatment.
fn regular_balanced(block_index: i64, cluster_value: &str, experiment_id: &str) -> bool {
    let seed = format!("{cluster_value}\x00{experiment_id}\x00group");
    let group =
        experimentation_hash::murmur3::murmurhash3_x86_32(seed.as_bytes(), 0) % 2;
    let base = simple_alternating(block_index);
    if group == 0 {
        base
    } else {
        !base
    }
}

/// RANDOMIZED: independent deterministic assignment per (block, cluster, experiment).
///
/// Uses `murmurhash3(experiment_id + cluster_value + block_index) % 2`.
fn randomized(block_index: i64, cluster_value: &str, experiment_id: &str) -> bool {
    let seed = format!("{experiment_id}\x00{cluster_value}\x00{block_index}");
    let hash = experimentation_hash::murmur3::murmurhash3_x86_32(seed.as_bytes(), 0);
    hash % 2 != 0
}

/// Validate a [`SwitchbackConfig`] against the M5 STARTING-phase constraints.
///
/// Returns `Ok(())` if the config is valid, or `Err(message)` with a human-
/// readable description of the first violation found.
pub fn validate_config(config: &SwitchbackConfig) -> Result<(), String> {
    if config.block_duration_secs < MIN_BLOCK_DURATION_SECS {
        return Err(format!(
            "switchback block_duration_secs ({}) must be >= {} (1 hour)",
            config.block_duration_secs, MIN_BLOCK_DURATION_SECS,
        ));
    }
    if config.planned_cycles < MIN_PLANNED_CYCLES {
        return Err(format!(
            "switchback planned_cycles ({}) must be >= {}",
            config.planned_cycles, MIN_PLANNED_CYCLES,
        ));
    }
    if config.washout_period_secs < 0 {
        return Err(format!(
            "switchback washout_period_secs ({}) must not be negative",
            config.washout_period_secs,
        ));
    }
    if config.washout_period_secs > 0
        && config.washout_period_secs >= config.block_duration_secs
    {
        return Err(format!(
            "switchback washout_period_secs ({}) must be < block_duration_secs ({})",
            config.washout_period_secs, config.block_duration_secs,
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{SwitchbackConfig, VariantConfig};

    fn control() -> VariantConfig {
        VariantConfig {
            variant_id: "control".into(),
            traffic_fraction: 0.5,
            is_control: true,
            payload_json: String::new(),
        }
    }

    fn treatment() -> VariantConfig {
        VariantConfig {
            variant_id: "treatment".into(),
            traffic_fraction: 0.5,
            is_control: false,
            payload_json: String::new(),
        }
    }

    // ── compute_block_index ──────────────────────────────────────────────────

    #[test]
    fn block_index_aligned_to_epoch() {
        // At t=0 block 0; at t=block_duration-1 still block 0; at t=block_duration block 1.
        let dur = 3600_i64; // 1 hour
        assert_eq!(compute_block_index(0, dur), 0);
        assert_eq!(compute_block_index(3599, dur), 0);
        assert_eq!(compute_block_index(3600, dur), 1);
        assert_eq!(compute_block_index(7199, dur), 1);
        assert_eq!(compute_block_index(7200, dur), 2);
    }

    #[test]
    fn block_index_zero_duration_guard() {
        assert_eq!(compute_block_index(999_999, 0), 0);
        assert_eq!(compute_block_index(999_999, -1), 0);
    }

    #[test]
    fn block_index_large_timestamp() {
        // 2026-01-01 00:00:00 UTC ≈ 1_767_225_600
        let t = 1_767_225_600_i64;
        let dur = 3600_i64;
        let expected = t / dur;
        assert_eq!(compute_block_index(t, dur), expected);
    }

    // ── is_in_washout ────────────────────────────────────────────────────────

    #[test]
    fn washout_at_block_start() {
        // Block duration 3600s, washout 300s (5 min).
        let dur = 3600_i64;
        let washout = 300_i64;
        // First 300s of any block are in washout.
        assert!(is_in_washout(0, dur, washout));
        assert!(is_in_washout(299, dur, washout));
        assert!(!is_in_washout(300, dur, washout));
        assert!(!is_in_washout(3599, dur, washout));
        // Second block starts at 3600.
        assert!(is_in_washout(3600, dur, washout));
        assert!(is_in_washout(3899, dur, washout));
        assert!(!is_in_washout(3900, dur, washout));
    }

    #[test]
    fn washout_zero_means_no_washout() {
        assert!(!is_in_washout(0, 3600, 0));
        assert!(!is_in_washout(100, 3600, 0));
    }

    #[test]
    fn washout_negative_duration_guard() {
        assert!(!is_in_washout(0, -1, 300));
        assert!(!is_in_washout(0, 3600, -1));
    }

    // ── select_variant — SIMPLE_ALTERNATING ─────────────────────────────────

    #[test]
    fn simple_alternating_even_block_is_control() {
        let variants = vec![control(), treatment()];
        let v = select_variant(0, "SIMPLE_ALTERNATING", "", "exp_1", &variants);
        assert_eq!(v.variant_id, "control");
        let v2 = select_variant(2, "SIMPLE_ALTERNATING", "", "exp_1", &variants);
        assert_eq!(v2.variant_id, "control");
    }

    #[test]
    fn simple_alternating_odd_block_is_treatment() {
        let variants = vec![control(), treatment()];
        let v = select_variant(1, "SIMPLE_ALTERNATING", "", "exp_1", &variants);
        assert_eq!(v.variant_id, "treatment");
        let v3 = select_variant(3, "SIMPLE_ALTERNATING", "", "exp_1", &variants);
        assert_eq!(v3.variant_id, "treatment");
    }

    #[test]
    fn default_design_behaves_as_simple_alternating() {
        let variants = vec![control(), treatment()];
        for block in 0..8_i64 {
            let expect = if block % 2 == 0 { "control" } else { "treatment" };
            let v = select_variant(block, "", "", "exp_1", &variants);
            assert_eq!(v.variant_id, expect, "block {block}");
        }
    }

    // ── select_variant — REGULAR_BALANCED ───────────────────────────────────

    #[test]
    fn regular_balanced_exactly_half_clusters_per_arm() {
        // Generate 1000 distinct cluster values and verify 50/50 split in each block.
        let variants = vec![control(), treatment()];
        for block in [0_i64, 1, 4, 99] {
            let mut counts = [0u32; 2];
            for i in 0..1000_u32 {
                let cluster = format!("cluster_{i}");
                let v = select_variant(block, "REGULAR_BALANCED", &cluster, "exp_bal", &variants);
                if v.variant_id == "control" {
                    counts[0] += 1;
                } else {
                    counts[1] += 1;
                }
            }
            // Expect ~500 each; allow ±5%.
            let diff = (counts[0] as i32 - counts[1] as i32).abs();
            assert!(
                diff < 100,
                "block {block}: unbalanced counts {counts:?} (diff {diff})"
            );
        }
    }

    #[test]
    fn regular_balanced_stagger_inverts_between_groups() {
        // Find two cluster values in different groups and verify they're always opposite.
        let variants = vec![control(), treatment()];
        // Search for one cluster in each group.
        let mut cluster_a = None;
        let mut cluster_b = None;
        for i in 0..200_u32 {
            let c = format!("c_{i}");
            let seed = format!("{c}\x00exp_stagger\x00group");
            let g = experimentation_hash::murmur3::murmurhash3_x86_32(seed.as_bytes(), 0) % 2;
            if g == 0 && cluster_a.is_none() {
                cluster_a = Some(c);
            } else if g == 1 && cluster_b.is_none() {
                cluster_b = Some(c);
            }
            if cluster_a.is_some() && cluster_b.is_some() {
                break;
            }
        }
        let ca = cluster_a.unwrap();
        let cb = cluster_b.unwrap();
        for block in 0..8_i64 {
            let va = select_variant(block, "REGULAR_BALANCED", &ca, "exp_stagger", &variants);
            let vb = select_variant(block, "REGULAR_BALANCED", &cb, "exp_stagger", &variants);
            assert_ne!(
                va.variant_id, vb.variant_id,
                "block {block}: clusters in different groups must be in opposite arms"
            );
        }
    }

    // ── select_variant — RANDOMIZED ─────────────────────────────────────────

    #[test]
    fn randomized_deterministic() {
        let variants = vec![control(), treatment()];
        let v1 = select_variant(42, "RANDOMIZED", "market_us", "exp_rand", &variants);
        let v2 = select_variant(42, "RANDOMIZED", "market_us", "exp_rand", &variants);
        assert_eq!(v1.variant_id, v2.variant_id);
    }

    #[test]
    fn randomized_approximately_balanced() {
        // Over 1000 block × cluster combinations, expect ~50/50.
        let variants = vec![control(), treatment()];
        let mut treatment_count = 0u32;
        for block in 0..100_i64 {
            for i in 0..10_u32 {
                let cluster = format!("m_{i}");
                let v = select_variant(block, "RANDOMIZED", &cluster, "exp_r", &variants);
                if v.variant_id == "treatment" {
                    treatment_count += 1;
                }
            }
        }
        // 1000 trials; expect 500 ± 50 (5%).
        let diff = (treatment_count as i32 - 500).abs();
        assert!(diff < 50, "randomized imbalance: {treatment_count}/1000 treatment");
    }

    // ── validate_config ──────────────────────────────────────────────────────

    #[test]
    fn validate_accepts_valid_config() {
        let cfg = SwitchbackConfig {
            block_duration_secs: 3600,
            planned_cycles: 4,
            cluster_attribute: "market_id".into(),
            washout_period_secs: 300,
            design: "SIMPLE_ALTERNATING".into(),
        };
        assert!(validate_config(&cfg).is_ok());
    }

    #[test]
    fn validate_rejects_short_block_duration() {
        let cfg = SwitchbackConfig {
            block_duration_secs: 1800, // 30 min — below 1-hour minimum
            planned_cycles: 4,
            ..Default::default()
        };
        let err = validate_config(&cfg).unwrap_err();
        assert!(err.contains("block_duration_secs"), "{err}");
    }

    #[test]
    fn validate_rejects_too_few_cycles() {
        let cfg = SwitchbackConfig {
            block_duration_secs: 3600,
            planned_cycles: 3, // below minimum of 4
            ..Default::default()
        };
        let err = validate_config(&cfg).unwrap_err();
        assert!(err.contains("planned_cycles"), "{err}");
    }

    #[test]
    fn validate_rejects_washout_gte_block_duration() {
        let cfg = SwitchbackConfig {
            block_duration_secs: 3600,
            planned_cycles: 4,
            washout_period_secs: 3600, // equal — must be strictly less
            ..Default::default()
        };
        let err = validate_config(&cfg).unwrap_err();
        assert!(err.contains("washout_period_secs"), "{err}");
    }

    #[test]
    fn validate_rejects_negative_washout() {
        let cfg = SwitchbackConfig {
            block_duration_secs: 3600,
            planned_cycles: 4,
            washout_period_secs: -1,
            ..Default::default()
        };
        let err = validate_config(&cfg).unwrap_err();
        assert!(err.contains("negative"), "{err}");
    }

    #[test]
    fn validate_accepts_zero_washout() {
        let cfg = SwitchbackConfig {
            block_duration_secs: 3600,
            planned_cycles: 4,
            washout_period_secs: 0,
            ..Default::default()
        };
        assert!(validate_config(&cfg).is_ok());
    }
}
