//! Team Draft Multileave (Schuth et al., 2015).
//!
//! N-way generalization of Team Draft interleaving for comparing 3+
//! ranking algorithms simultaneously. At each output position, the
//! team with the fewest contributions picks next (ties broken uniformly
//! at random). No floating-point computation.

use super::InterleavedResult;
use rand::Rng;
use std::collections::{HashMap, HashSet};

/// Produce an interleaved list using Team Draft Multileave.
///
/// # Arguments
/// * `lists` - Slice of `(ranked_items, algorithm_id)` per algorithm
/// * `k` - Maximum number of items in the merged list
/// * `rng` - Random number generator (deterministic seed for reproducibility)
pub fn multileave<R: Rng>(
    lists: &[(&[String], &str)],
    k: usize,
    rng: &mut R,
) -> InterleavedResult {
    let n = lists.len();
    let mut merged = Vec::with_capacity(k);
    let mut provenance = HashMap::new();
    let mut seen = HashSet::new();

    // Per-team state: current pointer into ranked list and contribution count.
    let mut pointers = vec![0usize; n];
    let mut counts = vec![0usize; n];

    while merged.len() < k {
        // Advance each team's pointer past already-seen items.
        for i in 0..n {
            while pointers[i] < lists[i].0.len() && seen.contains(&lists[i].0[pointers[i]]) {
                pointers[i] += 1;
            }
        }

        // Find eligible teams (those with remaining unseen items).
        let eligible: Vec<usize> = (0..n)
            .filter(|&i| pointers[i] < lists[i].0.len())
            .collect();

        if eligible.is_empty() {
            break;
        }

        // Among eligible teams, find the minimum contribution count.
        let min_count = eligible.iter().map(|&i| counts[i]).min().unwrap();

        // Collect teams tied at the minimum.
        let candidates: Vec<usize> = eligible
            .iter()
            .filter(|&&i| counts[i] == min_count)
            .copied()
            .collect();

        // Uniform random tie-breaking.
        let chosen_team = candidates[rng.gen_range(0..candidates.len())];

        let item = lists[chosen_team].0[pointers[chosen_team]].clone();
        seen.insert(item.clone());
        provenance.insert(item.clone(), lists[chosen_team].1.to_string());
        merged.push(item);
        counts[chosen_team] += 1;
        pointers[chosen_team] += 1;
    }

    InterleavedResult {
        merged_list: merged,
        provenance,
    }
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
    fn test_multileave_basic_3_lists() {
        let l1 = s(&["a", "b", "c"]);
        let l2 = s(&["d", "e", "f"]);
        let l3 = s(&["g", "h", "i"]);
        let mut rng = StdRng::seed_from_u64(42);

        let result = multileave(
            &[(&l1, "A"), (&l2, "B"), (&l3, "C")],
            9,
            &mut rng,
        );

        assert_eq!(result.merged_list.len(), 9);
        for item in &result.merged_list {
            assert!(result.provenance.contains_key(item));
            let algo = &result.provenance[item];
            assert!(["A", "B", "C"].contains(&algo.as_str()));
        }
    }

    #[test]
    fn test_multileave_deterministic() {
        let l1 = s(&["a", "b", "c"]);
        let l2 = s(&["d", "e", "f"]);
        let l3 = s(&["g", "h", "i"]);

        let mut rng1 = StdRng::seed_from_u64(12345);
        let mut rng2 = StdRng::seed_from_u64(12345);

        let r1 = multileave(&[(&l1, "A"), (&l2, "B"), (&l3, "C")], 9, &mut rng1);
        let r2 = multileave(&[(&l1, "A"), (&l2, "B"), (&l3, "C")], 9, &mut rng2);

        assert_eq!(r1.merged_list, r2.merged_list);
        assert_eq!(r1.provenance, r2.provenance);
    }

    #[test]
    fn test_multileave_balance_3_teams() {
        let l1 = s(&["a1", "a2", "a3", "a4", "a5"]);
        let l2 = s(&["b1", "b2", "b3", "b4", "b5"]);
        let l3 = s(&["c1", "c2", "c3", "c4", "c5"]);

        let mut totals = HashMap::new();

        for seed in 0..1000u64 {
            let mut rng = StdRng::seed_from_u64(seed);
            let result = multileave(
                &[(&l1, "A"), (&l2, "B"), (&l3, "C")],
                15,
                &mut rng,
            );
            for algo in result.provenance.values() {
                *totals.entry(algo.clone()).or_insert(0u64) += 1;
            }
        }

        let total: u64 = totals.values().sum();
        for (algo, count) in &totals {
            let frac = *count as f64 / total as f64;
            assert!(
                (0.25..=0.42).contains(&frac),
                "algo {algo} fraction {frac:.3} outside [0.25, 0.42]"
            );
        }
    }

    #[test]
    fn test_multileave_dedup_across_3() {
        let l1 = s(&["shared", "a_only"]);
        let l2 = s(&["shared", "b_only"]);
        let l3 = s(&["shared", "c_only"]);
        let mut rng = StdRng::seed_from_u64(0);

        let result = multileave(
            &[(&l1, "A"), (&l2, "B"), (&l3, "C")],
            10,
            &mut rng,
        );

        let shared_count = result.merged_list.iter().filter(|i| *i == "shared").count();
        assert_eq!(shared_count, 1, "shared item should appear exactly once");
        assert_eq!(result.merged_list.len(), 4); // shared + a_only + b_only + c_only
    }

    #[test]
    fn test_multileave_empty_lists() {
        let l1: Vec<String> = vec![];
        let l2: Vec<String> = vec![];
        let l3: Vec<String> = vec![];
        let mut rng = StdRng::seed_from_u64(0);

        let result = multileave(
            &[(&l1, "A"), (&l2, "B"), (&l3, "C")],
            10,
            &mut rng,
        );

        assert!(result.merged_list.is_empty());
        assert!(result.provenance.is_empty());
    }

    #[test]
    fn test_multileave_one_empty_among_three() {
        let l1 = s(&["a", "b", "c"]);
        let l2: Vec<String> = vec![];
        let l3 = s(&["d", "e", "f"]);
        let mut rng = StdRng::seed_from_u64(42);

        let result = multileave(
            &[(&l1, "A"), (&l2, "B"), (&l3, "C")],
            10,
            &mut rng,
        );

        assert_eq!(result.merged_list.len(), 6);
        // Only A and C should have provenance.
        for algo in result.provenance.values() {
            assert!(algo == "A" || algo == "C", "unexpected algo: {algo}");
        }
    }

    #[test]
    fn test_multileave_k_smaller_than_total() {
        let l1 = s(&["a", "b", "c", "d", "e"]);
        let l2 = s(&["f", "g", "h", "i", "j"]);
        let l3 = s(&["k", "l", "m", "n", "o"]);
        let mut rng = StdRng::seed_from_u64(42);

        let result = multileave(
            &[(&l1, "A"), (&l2, "B"), (&l3, "C")],
            5,
            &mut rng,
        );

        assert_eq!(result.merged_list.len(), 5);
    }

    #[test]
    fn test_multileave_4_algorithms() {
        let l1 = s(&["a1", "a2", "a3"]);
        let l2 = s(&["b1", "b2", "b3"]);
        let l3 = s(&["c1", "c2", "c3"]);
        let l4 = s(&["d1", "d2", "d3"]);
        let mut rng = StdRng::seed_from_u64(42);

        let result = multileave(
            &[(&l1, "A"), (&l2, "B"), (&l3, "C"), (&l4, "D")],
            12,
            &mut rng,
        );

        assert_eq!(result.merged_list.len(), 12);
        let algos: HashSet<_> = result.provenance.values().collect();
        assert_eq!(algos.len(), 4, "all 4 algorithms should contribute");
    }

    #[test]
    fn test_multileave_5_algorithms_large() {
        let lists: Vec<Vec<String>> = (0..5)
            .map(|team| (0..20).map(|i| format!("t{team}_item_{i}")).collect())
            .collect();
        let mut rng = StdRng::seed_from_u64(42);

        let refs: Vec<(&[String], &str)> = vec![
            (&lists[0], "A"),
            (&lists[1], "B"),
            (&lists[2], "C"),
            (&lists[3], "D"),
            (&lists[4], "E"),
        ];

        let result = multileave(&refs, 50, &mut rng);

        assert_eq!(result.merged_list.len(), 50); // 5 × 20 = 100 items, k=50
        let unique: HashSet<_> = result.merged_list.iter().collect();
        assert_eq!(unique.len(), 50, "no duplicates in merged list");

        let algos: HashSet<_> = result.provenance.values().collect();
        assert_eq!(algos.len(), 5, "all 5 algorithms should contribute");
    }

    #[test]
    fn test_multileave_single_item_lists() {
        let l1 = s(&["only_a"]);
        let l2 = s(&["only_b"]);
        let l3 = s(&["only_c"]);
        let mut rng = StdRng::seed_from_u64(99);

        let result = multileave(
            &[(&l1, "A"), (&l2, "B"), (&l3, "C")],
            10,
            &mut rng,
        );

        assert_eq!(result.merged_list.len(), 3);
        assert!(result.merged_list.contains(&"only_a".to_string()));
        assert!(result.merged_list.contains(&"only_b".to_string()));
        assert!(result.merged_list.contains(&"only_c".to_string()));
    }
}
