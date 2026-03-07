//! Team Draft interleaving (Radlinski et al., 2008).

use super::InterleavedResult;
use rand::Rng;
use std::collections::{HashMap, HashSet};

/// Produce an interleaved list using the Team Draft algorithm.
///
/// # Arguments
/// * `list_a` - Ranked items from algorithm A (index 0 = top rank)
/// * `list_b` - Ranked items from algorithm B
/// * `algo_a_id` - Identifier for algorithm A
/// * `algo_b_id` - Identifier for algorithm B
/// * `k` - Maximum number of items in the merged list
pub fn team_draft<R: Rng>(
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

    let mut idx_a = 0;
    let mut idx_b = 0;
    let mut team_a_count = 0;
    let mut team_b_count = 0;

    while merged.len() < k && (idx_a < list_a.len() || idx_b < list_b.len()) {
        // Advance past already-seen items.
        while idx_a < list_a.len() && seen.contains(&list_a[idx_a]) {
            idx_a += 1;
        }
        while idx_b < list_b.len() && seen.contains(&list_b[idx_b]) {
            idx_b += 1;
        }

        let a_has_items = idx_a < list_a.len();
        let b_has_items = idx_b < list_b.len();
        if !a_has_items && !b_has_items {
            break;
        }

        // Determine which team picks next (fallback to the team with remaining items).
        let a_picks = if !a_has_items {
            false
        } else if !b_has_items || team_a_count < team_b_count {
            true
        } else if team_b_count < team_a_count {
            false
        } else {
            rng.gen_bool(0.5)
        };

        if a_picks {
            let item = list_a[idx_a].clone();
            seen.insert(item.clone());
            provenance.insert(item.clone(), algo_a_id.to_string());
            merged.push(item);
            team_a_count += 1;
            idx_a += 1;
        } else {
            let item = list_b[idx_b].clone();
            seen.insert(item.clone());
            provenance.insert(item.clone(), algo_b_id.to_string());
            merged.push(item);
            team_b_count += 1;
            idx_b += 1;
        }
    }

    InterleavedResult { merged_list: merged, provenance }
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
    fn test_team_draft_basic() {
        let list_a = s(&["a", "b", "c", "d"]);
        let list_b = s(&["c", "a", "d", "b"]);
        let mut rng = rand::thread_rng();

        let result = team_draft(&list_a, &list_b, "algo_a", "algo_b", 4, &mut rng);

        assert_eq!(result.merged_list.len(), 4);
        for item in &result.merged_list {
            assert!(result.provenance.contains_key(item));
        }
    }

    #[test]
    fn test_team_draft_deterministic_seed() {
        let list_a = s(&["a", "b", "c"]);
        let list_b = s(&["d", "e", "f"]);

        let mut rng1 = StdRng::seed_from_u64(12345);
        let mut rng2 = StdRng::seed_from_u64(12345);

        let r1 = team_draft(&list_a, &list_b, "A", "B", 6, &mut rng1);
        let r2 = team_draft(&list_a, &list_b, "A", "B", 6, &mut rng2);

        assert_eq!(r1.merged_list, r2.merged_list);
        assert_eq!(r1.provenance, r2.provenance);
    }

    #[test]
    fn test_team_draft_k_smaller_than_inputs() {
        let list_a = s(&["a", "b", "c", "d", "e"]);
        let list_b = s(&["f", "g", "h", "i", "j"]);
        let mut rng = StdRng::seed_from_u64(42);

        let result = team_draft(&list_a, &list_b, "A", "B", 3, &mut rng);
        assert_eq!(result.merged_list.len(), 3);
    }

    #[test]
    fn test_team_draft_single_item_lists() {
        let list_a = s(&["only_a"]);
        let list_b = s(&["only_b"]);
        let mut rng = StdRng::seed_from_u64(99);

        let result = team_draft(&list_a, &list_b, "A", "B", 10, &mut rng);
        assert_eq!(result.merged_list.len(), 2);
        assert!(result.merged_list.contains(&"only_a".to_string()));
        assert!(result.merged_list.contains(&"only_b".to_string()));
    }

    #[test]
    fn test_team_draft_one_empty_list() {
        let list_a = s(&["a", "b", "c"]);
        let list_b: Vec<String> = vec![];
        let mut rng = StdRng::seed_from_u64(7);

        let result = team_draft(&list_a, &list_b, "A", "B", 5, &mut rng);
        assert_eq!(result.merged_list.len(), 3);
        for item in &result.merged_list {
            assert_eq!(result.provenance[item], "A");
        }
    }

    #[test]
    fn test_team_draft_overlap_dedup() {
        // Both lists share "shared". First team to pick gets provenance.
        let list_a = s(&["shared", "a_only"]);
        let list_b = s(&["shared", "b_only"]);
        let mut rng = StdRng::seed_from_u64(0);

        let result = team_draft(&list_a, &list_b, "A", "B", 10, &mut rng);

        let shared_count = result.merged_list.iter().filter(|i| *i == "shared").count();
        assert_eq!(shared_count, 1, "shared item should appear exactly once");
        assert_eq!(result.merged_list.len(), 3); // shared + a_only + b_only
    }

    #[test]
    fn test_team_draft_balance_property() {
        // Over many trials with different seeds, both teams should contribute ~50%.
        let list_a = s(&["i1", "i2", "i3", "i4", "i5"]);
        let list_b = s(&["i6", "i7", "i8", "i9", "i10"]);

        let mut team_a_total = 0u64;
        let mut team_b_total = 0u64;

        for seed in 0..1000u64 {
            let mut rng = StdRng::seed_from_u64(seed);
            let result = team_draft(&list_a, &list_b, "A", "B", 10, &mut rng);
            for algo in result.provenance.values() {
                match algo.as_str() {
                    "A" => team_a_total += 1,
                    "B" => team_b_total += 1,
                    _ => panic!("unexpected"),
                }
            }
        }

        let total = (team_a_total + team_b_total) as f64;
        let frac_a = team_a_total as f64 / total;
        assert!(
            (0.45..=0.55).contains(&frac_a),
            "team A fraction {frac_a:.3} is outside [0.45, 0.55]"
        );
    }
}
