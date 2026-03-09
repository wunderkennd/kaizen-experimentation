//! Optimized Interleaving (Radlinski & Craswell, WSDM 2010).
//!
//! Greedy softmax approximation that maximizes sensitivity by using
//! position-weighted probabilistic selection instead of Team Draft's
//! uniform coin flip.

use super::InterleavedResult;
use rand::Rng;
use std::collections::{HashMap, HashSet};

/// Candidate item ready for softmax scoring.
struct Candidate {
    item: String,
    /// Which source list this item comes from (0 = A, 1 = B).
    source: usize,
    /// Rank within the source list (0-based).
    rank: usize,
}

/// Produce an interleaved list using the Optimized Interleaving algorithm.
///
/// At each output position, scores all eligible candidates using:
/// - **Rank quality**: `1.0 / (rank_in_source + 1)` — higher source rank → higher score
/// - **Position weight**: `1.0 / log2(pos + 2)` — DCG-style discount for later positions
/// - **Balance bonus**: `±0.5` — bonus for underrepresented list
///
/// Candidates are selected via numerically stable softmax (log-sum-exp trick).
/// A hard balance cap of `ceil(k/2)` prevents either list from dominating.
///
/// # Arguments
/// * `list_a` - Ranked items from algorithm A (index 0 = top rank)
/// * `list_b` - Ranked items from algorithm B
/// * `algo_a_id` - Identifier for algorithm A
/// * `algo_b_id` - Identifier for algorithm B
/// * `k` - Maximum number of items in the merged list
/// * `rng` - Random number generator (deterministic seed for reproducibility)
pub fn optimized_interleave<R: Rng>(
    list_a: &[String],
    list_b: &[String],
    algo_a_id: &str,
    algo_b_id: &str,
    k: usize,
    rng: &mut R,
) -> InterleavedResult {
    let mut merged = Vec::with_capacity(k);
    let mut provenance = HashMap::new();
    let mut seen = HashSet::new();
    let mut count = [0usize; 2]; // items picked from list A, list B
    let balance_cap = k.div_ceil(2);

    let algo_ids = [algo_a_id, algo_b_id];

    for pos in 0..k {
        // Build candidate set: unseen items from both lists, respecting balance cap.
        let candidates = build_candidates(list_a, list_b, &seen, &count, balance_cap);
        if candidates.is_empty() {
            break;
        }

        // Score each candidate.
        let scores: Vec<f64> = candidates
            .iter()
            .map(|c| score_candidate(c, pos, &count))
            .collect();

        // Softmax selection.
        let idx = softmax_sample(&scores, rng);
        let chosen = &candidates[idx];

        seen.insert(chosen.item.clone());
        provenance.insert(chosen.item.clone(), algo_ids[chosen.source].to_string());
        merged.push(chosen.item.clone());
        count[chosen.source] += 1;
    }

    InterleavedResult {
        merged_list: merged,
        provenance,
    }
}

/// Build the set of eligible candidates from both lists.
fn build_candidates(
    list_a: &[String],
    list_b: &[String],
    seen: &HashSet<String>,
    count: &[usize; 2],
    balance_cap: usize,
) -> Vec<Candidate> {
    let mut candidates = Vec::new();
    let lists: [&[String]; 2] = [list_a, list_b];

    for (source, list) in lists.iter().enumerate() {
        if count[source] >= balance_cap {
            continue; // Hard cap reached for this list.
        }
        for (rank, item) in list.iter().enumerate() {
            if !seen.contains(item) {
                candidates.push(Candidate {
                    item: item.clone(),
                    source,
                    rank,
                });
            }
        }
    }

    // Dedup: if the same item appears from both lists, keep the first occurrence
    // (lower source index = list A preferred in tie).
    let mut dedup_seen = HashSet::new();
    candidates.retain(|c| dedup_seen.insert(c.item.clone()));

    candidates
}

/// Score a candidate for position `pos` in the output.
fn score_candidate(c: &Candidate, pos: usize, count: &[usize; 2]) -> f64 {
    let rank_quality = 1.0 / (c.rank as f64 + 1.0);
    let position_weight = 1.0 / ((pos + 2) as f64).log2();

    // Balance bonus: +0.5 if this list is underrepresented, -0.5 if overrepresented.
    let other = 1 - c.source;
    let balance_bonus = if count[c.source] < count[other] {
        0.5
    } else if count[c.source] > count[other] {
        -0.5
    } else {
        0.0
    };

    rank_quality * position_weight + balance_bonus
}

/// Numerically stable softmax sampling using the log-sum-exp trick.
///
/// Returns the index of the sampled candidate.
fn softmax_sample<R: Rng>(scores: &[f64], rng: &mut R) -> usize {
    debug_assert!(!scores.is_empty());

    if scores.len() == 1 {
        return 0;
    }

    // Log-sum-exp trick for numerical stability.
    let max_score = scores.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let exp_scores: Vec<f64> = scores.iter().map(|s| (s - max_score).exp()).collect();
    let sum_exp: f64 = exp_scores.iter().sum();

    // Sample from the probability distribution.
    let threshold = rng.gen::<f64>() * sum_exp;
    let mut cumulative = 0.0;
    for (i, &e) in exp_scores.iter().enumerate() {
        cumulative += e;
        if cumulative >= threshold {
            return i;
        }
    }

    // Fallthrough (floating-point edge case).
    scores.len() - 1
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::StdRng;
    use rand::SeedableRng;

    fn s(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn test_optimized_basic() {
        let list_a = s(&["a", "b", "c", "d"]);
        let list_b = s(&["e", "f", "g", "h"]);
        let mut rng = StdRng::seed_from_u64(42);

        let result = optimized_interleave(&list_a, &list_b, "A", "B", 6, &mut rng);

        assert!(!result.merged_list.is_empty());
        assert!(result.merged_list.len() <= 6);
        for item in &result.merged_list {
            assert!(result.provenance.contains_key(item));
        }
    }

    #[test]
    fn test_optimized_deterministic() {
        let list_a = s(&["a", "b", "c"]);
        let list_b = s(&["d", "e", "f"]);

        let mut rng1 = StdRng::seed_from_u64(12345);
        let mut rng2 = StdRng::seed_from_u64(12345);

        let r1 = optimized_interleave(&list_a, &list_b, "A", "B", 6, &mut rng1);
        let r2 = optimized_interleave(&list_a, &list_b, "A", "B", 6, &mut rng2);

        assert_eq!(r1.merged_list, r2.merged_list);
        assert_eq!(r1.provenance, r2.provenance);
    }

    #[test]
    fn test_optimized_balance() {
        // Over many seeds, both lists should contribute roughly equally.
        let list_a = s(&["i1", "i2", "i3", "i4", "i5"]);
        let list_b = s(&["i6", "i7", "i8", "i9", "i10"]);

        let mut a_total = 0u64;
        let mut b_total = 0u64;

        for seed in 0..1000u64 {
            let mut rng = StdRng::seed_from_u64(seed);
            let result = optimized_interleave(&list_a, &list_b, "A", "B", 10, &mut rng);
            for algo in result.provenance.values() {
                match algo.as_str() {
                    "A" => a_total += 1,
                    "B" => b_total += 1,
                    _ => panic!("unexpected"),
                }
            }
        }

        let total = (a_total + b_total) as f64;
        let frac_a = a_total as f64 / total;
        assert!(
            (0.35..=0.65).contains(&frac_a),
            "list A fraction {frac_a:.3} is outside [0.35, 0.65]"
        );
    }

    #[test]
    fn test_optimized_single_item() {
        let list_a = s(&["only_a"]);
        let list_b = s(&["only_b"]);
        let mut rng = StdRng::seed_from_u64(99);

        let result = optimized_interleave(&list_a, &list_b, "A", "B", 10, &mut rng);
        assert_eq!(result.merged_list.len(), 2);
        assert!(result.merged_list.contains(&"only_a".to_string()));
        assert!(result.merged_list.contains(&"only_b".to_string()));
    }

    #[test]
    fn test_optimized_empty_lists() {
        let list_a: Vec<String> = vec![];
        let list_b: Vec<String> = vec![];
        let mut rng = StdRng::seed_from_u64(7);

        let result = optimized_interleave(&list_a, &list_b, "A", "B", 10, &mut rng);
        assert!(result.merged_list.is_empty());
        assert!(result.provenance.is_empty());
    }

    #[test]
    fn test_optimized_overlap_dedup() {
        let list_a = s(&["shared", "a_only"]);
        let list_b = s(&["shared", "b_only"]);
        let mut rng = StdRng::seed_from_u64(0);

        let result = optimized_interleave(&list_a, &list_b, "A", "B", 10, &mut rng);
        let shared_count = result.merged_list.iter().filter(|i| *i == "shared").count();
        assert_eq!(shared_count, 1, "shared item should appear exactly once");
        assert_eq!(result.merged_list.len(), 3);
    }

    #[test]
    fn test_optimized_k_limits() {
        let list_a = s(&["a", "b", "c", "d", "e"]);
        let list_b = s(&["f", "g", "h", "i", "j"]);
        let mut rng = StdRng::seed_from_u64(42);

        let result = optimized_interleave(&list_a, &list_b, "A", "B", 3, &mut rng);
        assert_eq!(result.merged_list.len(), 3);
    }

    #[test]
    fn test_optimized_softmax_numerical_stability() {
        // Large rank differences shouldn't cause NaN/Infinity.
        let list_a: Vec<String> = (0..100).map(|i| format!("a_{i}")).collect();
        let list_b: Vec<String> = (0..100).map(|i| format!("b_{i}")).collect();
        let mut rng = StdRng::seed_from_u64(42);

        let result = optimized_interleave(&list_a, &list_b, "A", "B", 50, &mut rng);
        assert_eq!(result.merged_list.len(), 50);
        // No duplicate items.
        let unique: HashSet<_> = result.merged_list.iter().collect();
        assert_eq!(unique.len(), 50);
    }

    #[test]
    fn test_optimized_favors_higher_ranked() {
        // Top-ranked items from both lists should appear more often in early positions.
        let list_a = s(&["top_a", "mid_a", "low_a"]);
        let list_b = s(&["top_b", "mid_b", "low_b"]);

        let mut top_in_first_two = 0u64;
        let trials = 1000u64;

        for seed in 0..trials {
            let mut rng = StdRng::seed_from_u64(seed);
            let result = optimized_interleave(&list_a, &list_b, "A", "B", 6, &mut rng);
            // Check if "top_a" or "top_b" appears in the first 2 positions.
            for item in result.merged_list.iter().take(2) {
                if item == "top_a" || item == "top_b" {
                    top_in_first_two += 1;
                }
            }
        }

        // With rank-quality weighting, top items should appear in first 2 positions
        // much more than random (random = 2/6 * 2 * 1000 ≈ 667).
        assert!(
            top_in_first_two > 800,
            "top items appeared {top_in_first_two}/{trials} times in first 2 positions — expected > 800"
        );
    }

    #[test]
    fn test_optimized_balance_cap() {
        // With k=4, balance cap = ceil(4/2) = 2. Neither list can contribute > 2 items.
        let list_a = s(&["a1", "a2", "a3"]);
        let list_b = s(&["b1", "b2", "b3"]);

        for seed in 0..500u64 {
            let mut rng = StdRng::seed_from_u64(seed);
            let result = optimized_interleave(&list_a, &list_b, "A", "B", 4, &mut rng);

            let a_count = result.provenance.values().filter(|v| *v == "A").count();
            let b_count = result.provenance.values().filter(|v| *v == "B").count();
            assert!(a_count <= 2, "seed {seed}: list A contributed {a_count} > cap 2");
            assert!(b_count <= 2, "seed {seed}: list B contributed {b_count} > cap 2");
        }
    }

    #[test]
    fn test_optimized_all_overlap() {
        // Both lists have the same items — should produce k unique items.
        let list_a = s(&["x", "y", "z"]);
        let list_b = s(&["x", "y", "z"]);
        let mut rng = StdRng::seed_from_u64(42);

        let result = optimized_interleave(&list_a, &list_b, "A", "B", 3, &mut rng);
        assert_eq!(result.merged_list.len(), 3);
        let unique: HashSet<_> = result.merged_list.iter().collect();
        assert_eq!(unique.len(), 3);
    }

    #[test]
    fn test_optimized_large_lists() {
        let list_a: Vec<String> = (0..500).map(|i| format!("a_{i}")).collect();
        let list_b: Vec<String> = (0..500).map(|i| format!("b_{i}")).collect();
        let mut rng = StdRng::seed_from_u64(42);

        let result = optimized_interleave(&list_a, &list_b, "A", "B", 100, &mut rng);
        assert_eq!(result.merged_list.len(), 100);
        let unique: HashSet<_> = result.merged_list.iter().collect();
        assert_eq!(unique.len(), 100);
    }
}
