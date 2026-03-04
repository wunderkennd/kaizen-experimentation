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
        // Determine which team picks next.
        let a_picks = if team_a_count < team_b_count {
            true
        } else if team_b_count < team_a_count {
            false
        } else {
            rng.gen_bool(0.5)
        };

        if a_picks {
            while idx_a < list_a.len() && seen.contains(&list_a[idx_a]) {
                idx_a += 1;
            }
            if idx_a < list_a.len() {
                let item = list_a[idx_a].clone();
                seen.insert(item.clone());
                provenance.insert(item.clone(), algo_a_id.to_string());
                merged.push(item);
                team_a_count += 1;
                idx_a += 1;
            }
        } else {
            while idx_b < list_b.len() && seen.contains(&list_b[idx_b]) {
                idx_b += 1;
            }
            if idx_b < list_b.len() {
                let item = list_b[idx_b].clone();
                seen.insert(item.clone());
                provenance.insert(item.clone(), algo_b_id.to_string());
                merged.push(item);
                team_b_count += 1;
                idx_b += 1;
            }
        }
    }

    InterleavedResult { merged_list: merged, provenance }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_team_draft_basic() {
        let list_a: Vec<String> = vec!["a", "b", "c", "d"].into_iter().map(String::from).collect();
        let list_b: Vec<String> = vec!["c", "a", "d", "b"].into_iter().map(String::from).collect();
        let mut rng = rand::thread_rng();

        let result = team_draft(&list_a, &list_b, "algo_a", "algo_b", 4, &mut rng);

        assert_eq!(result.merged_list.len(), 4);
        // Every item should have provenance.
        for item in &result.merged_list {
            assert!(result.provenance.contains_key(item));
        }
    }
}
