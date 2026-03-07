//! Interference analysis for content recommendation experiments.
//!
//! Detects whether treatment effects spill over to control group content
//! consumption patterns (e.g., algorithmic recommendations change what
//! control users see indirectly).
//!
//! Metrics:
//! - **Jensen-Shannon divergence**: Distribution similarity [0, ln(2)]
//! - **Jaccard similarity**: Top-K content overlap [0, 1]
//! - **Gini coefficient**: Concentration inequality [0, 1]
//! - **Title spillover**: Per-title two-proportion z-test with BH correction
//!
//! See design doc section 7.4 for specification.

use std::collections::{HashMap, HashSet};

use experimentation_core::error::{assert_finite, Error, Result};

use crate::multiple_comparison::benjamini_hochberg;

/// Content consumption data for a single title.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ContentConsumption {
    pub content_id: String,
    pub watch_time_seconds: f64,
    pub view_count: u64,
    pub unique_viewers: u64,
}

/// Input for interference analysis.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InterferenceInput {
    pub treatment: Vec<ContentConsumption>,
    pub control: Vec<ContentConsumption>,
    pub total_treatment_viewers: u64,
    pub total_control_viewers: u64,
}

/// Per-title spillover test result.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TitleSpillover {
    pub content_id: String,
    pub treatment_watch_rate: f64,
    pub control_watch_rate: f64,
    pub p_value: f64,
}

/// Full interference analysis result.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InterferenceAnalysisResult {
    pub interference_detected: bool,
    pub jensen_shannon_divergence: f64,
    pub jaccard_similarity_top_100: f64,
    pub treatment_gini_coefficient: f64,
    pub control_gini_coefficient: f64,
    pub treatment_catalog_coverage: f64,
    pub control_catalog_coverage: f64,
    pub spillover_titles: Vec<TitleSpillover>,
}

/// Run full interference analysis.
///
/// `js_threshold` — JSD value above which interference is flagged (typical: 0.05).
pub fn analyze_interference(
    input: &InterferenceInput,
    alpha: f64,
    js_threshold: f64,
) -> Result<InterferenceAnalysisResult> {
    if alpha <= 0.0 || alpha >= 1.0 {
        return Err(Error::Validation("alpha must be in (0, 1)".into()));
    }
    if js_threshold <= 0.0 {
        return Err(Error::Validation(
            "js_threshold must be positive".into(),
        ));
    }
    if input.treatment.is_empty() || input.control.is_empty() {
        return Err(Error::Validation(
            "treatment and control must not be empty".into(),
        ));
    }
    if input.total_treatment_viewers == 0 || input.total_control_viewers == 0 {
        return Err(Error::Validation(
            "total viewers must be positive".into(),
        ));
    }

    // Build watch-time distributions over the union of content IDs.
    let t_total: f64 = input.treatment.iter().map(|c| c.watch_time_seconds).sum();
    let c_total: f64 = input.control.iter().map(|c| c.watch_time_seconds).sum();
    assert_finite(t_total, "treatment_total_watch_time");
    assert_finite(c_total, "control_total_watch_time");

    if t_total <= 0.0 || c_total <= 0.0 {
        return Err(Error::Validation(
            "total watch time must be positive for both groups".into(),
        ));
    }

    let t_map: HashMap<&str, f64> = input
        .treatment
        .iter()
        .map(|c| (c.content_id.as_str(), c.watch_time_seconds))
        .collect();
    let c_map: HashMap<&str, f64> = input
        .control
        .iter()
        .map(|c| (c.content_id.as_str(), c.watch_time_seconds))
        .collect();

    // Union of all content IDs.
    let all_ids: HashSet<&str> = t_map.keys().chain(c_map.keys()).copied().collect();
    let total_catalog = all_ids.len() as f64;

    // Normalized distributions.
    let mut p_vec = Vec::with_capacity(all_ids.len());
    let mut q_vec = Vec::with_capacity(all_ids.len());
    for &id in &all_ids {
        let p = t_map.get(id).copied().unwrap_or(0.0) / t_total;
        let q = c_map.get(id).copied().unwrap_or(0.0) / c_total;
        assert_finite(p, &format!("p_dist[{id}]"));
        assert_finite(q, &format!("q_dist[{id}]"));
        p_vec.push(p);
        q_vec.push(q);
    }

    let jsd = jensen_shannon_divergence(&p_vec, &q_vec);
    assert_finite(jsd, "jensen_shannon_divergence");

    let jaccard = jaccard_similarity_top_k(&input.treatment, &input.control, 100);
    assert_finite(jaccard, "jaccard_similarity_top_100");

    let t_watch_times: Vec<f64> = input.treatment.iter().map(|c| c.watch_time_seconds).collect();
    let c_watch_times: Vec<f64> = input.control.iter().map(|c| c.watch_time_seconds).collect();

    let t_gini = gini_coefficient(&t_watch_times);
    assert_finite(t_gini, "treatment_gini");
    let c_gini = gini_coefficient(&c_watch_times);
    assert_finite(c_gini, "control_gini");

    let t_coverage = input.treatment.len() as f64 / total_catalog;
    let c_coverage = input.control.len() as f64 / total_catalog;
    assert_finite(t_coverage, "treatment_catalog_coverage");
    assert_finite(c_coverage, "control_catalog_coverage");

    let spillover = title_spillover_test(
        &input.treatment,
        &input.control,
        input.total_treatment_viewers,
        input.total_control_viewers,
        alpha,
    )?;

    let interference_detected = jsd > js_threshold || !spillover.is_empty();

    Ok(InterferenceAnalysisResult {
        interference_detected,
        jensen_shannon_divergence: jsd,
        jaccard_similarity_top_100: jaccard,
        treatment_gini_coefficient: t_gini,
        control_gini_coefficient: c_gini,
        treatment_catalog_coverage: t_coverage,
        control_catalog_coverage: c_coverage,
        spillover_titles: spillover,
    })
}

/// Jensen-Shannon divergence: JSD(P || Q) = (KL(P || M) + KL(Q || M)) / 2
/// where M = (P + Q) / 2. Range [0, ln(2)].
fn jensen_shannon_divergence(p: &[f64], q: &[f64]) -> f64 {
    debug_assert_eq!(p.len(), q.len());
    let mut kl_pm = 0.0;
    let mut kl_qm = 0.0;
    for (&pi, &qi) in p.iter().zip(q.iter()) {
        let m = (pi + qi) / 2.0;
        if pi > 0.0 && m > 0.0 {
            kl_pm += pi * (pi / m).ln();
        }
        if qi > 0.0 && m > 0.0 {
            kl_qm += qi * (qi / m).ln();
        }
    }
    (kl_pm + kl_qm) / 2.0
}

/// Jaccard similarity of top-K content IDs by watch time.
fn jaccard_similarity_top_k(
    treatment: &[ContentConsumption],
    control: &[ContentConsumption],
    k: usize,
) -> f64 {
    let top_t = top_k_ids(treatment, k);
    let top_c = top_k_ids(control, k);
    let intersection = top_t.intersection(&top_c).count();
    let union = top_t.union(&top_c).count();
    if union == 0 {
        return 1.0;
    }
    intersection as f64 / union as f64
}

fn top_k_ids(items: &[ContentConsumption], k: usize) -> HashSet<String> {
    let mut sorted: Vec<&ContentConsumption> = items.iter().collect();
    sorted.sort_by(|a, b| {
        b.watch_time_seconds
            .partial_cmp(&a.watch_time_seconds)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    sorted
        .into_iter()
        .take(k)
        .map(|c| c.content_id.clone())
        .collect()
}

/// Gini coefficient from a set of values.
/// G = (2·sum(i·x[i])) / (n·sum(x)) - (n+1)/n where values are sorted ascending.
fn gini_coefficient(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted: Vec<f64> = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let n = sorted.len() as f64;
    let total: f64 = sorted.iter().sum();
    if total <= 0.0 {
        return 0.0;
    }

    let weighted_sum: f64 = sorted
        .iter()
        .enumerate()
        .map(|(i, &x)| (i as f64 + 1.0) * x)
        .sum();

    (2.0 * weighted_sum) / (n * total) - (n + 1.0) / n
}

/// Per-title two-proportion z-test with Benjamini-Hochberg correction.
fn title_spillover_test(
    treatment: &[ContentConsumption],
    control: &[ContentConsumption],
    total_t: u64,
    total_c: u64,
    alpha: f64,
) -> Result<Vec<TitleSpillover>> {
    // Build maps of unique_viewers per content_id.
    let t_map: HashMap<&str, u64> = treatment
        .iter()
        .map(|c| (c.content_id.as_str(), c.unique_viewers))
        .collect();
    let c_map: HashMap<&str, u64> = control
        .iter()
        .map(|c| (c.content_id.as_str(), c.unique_viewers))
        .collect();

    // Union of content IDs that appear in both groups.
    let shared_ids: Vec<&str> = t_map
        .keys()
        .filter(|&&id| c_map.contains_key(id))
        .copied()
        .collect();

    if shared_ids.is_empty() {
        return Ok(Vec::new());
    }

    let nt = total_t as f64;
    let nc = total_c as f64;

    let mut p_values = Vec::with_capacity(shared_ids.len());
    let mut rates = Vec::with_capacity(shared_ids.len());

    for &id in &shared_ids {
        let x_t = *t_map.get(id).unwrap() as f64;
        let x_c = *c_map.get(id).unwrap() as f64;
        let p_t = x_t / nt;
        let p_c = x_c / nc;
        assert_finite(p_t, &format!("spillover_rate_t[{id}]"));
        assert_finite(p_c, &format!("spillover_rate_c[{id}]"));

        // Pooled proportion for two-proportion z-test.
        let p_pool = (x_t + x_c) / (nt + nc);
        assert_finite(p_pool, &format!("spillover_pooled[{id}]"));

        if p_pool <= 0.0 || p_pool >= 1.0 {
            // Degenerate case — all or none viewed.
            p_values.push(1.0);
            rates.push((id, p_t, p_c));
            continue;
        }

        let se = (p_pool * (1.0 - p_pool) * (1.0 / nt + 1.0 / nc)).sqrt();
        assert_finite(se, &format!("spillover_se[{id}]"));

        if se <= 0.0 {
            p_values.push(1.0);
            rates.push((id, p_t, p_c));
            continue;
        }

        let z = (p_t - p_c) / se;
        assert_finite(z, &format!("spillover_z[{id}]"));

        // Two-sided p-value from standard normal.
        let p_value = 2.0 * normal_cdf(-z.abs());
        assert_finite(p_value, &format!("spillover_pvalue[{id}]"));

        p_values.push(p_value.clamp(0.0, 1.0));
        rates.push((id, p_t, p_c));
    }

    // BH correction.
    let bh = benjamini_hochberg(&p_values, alpha)?;

    let mut spillover = Vec::new();
    for (i, &rejected) in bh.rejected.iter().enumerate() {
        if rejected {
            let (id, p_t, p_c) = rates[i];
            spillover.push(TitleSpillover {
                content_id: id.to_string(),
                treatment_watch_rate: p_t,
                control_watch_rate: p_c,
                p_value: bh.p_values_adjusted[i],
            });
        }
    }

    Ok(spillover)
}

/// Standard normal CDF via error function approximation.
fn normal_cdf(x: f64) -> f64 {
    use statrs::distribution::{ContinuousCDF, Normal};
    let n = Normal::new(0.0, 1.0).unwrap();
    n.cdf(x)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_content(id: &str, watch_time: f64, views: u64, viewers: u64) -> ContentConsumption {
        ContentConsumption {
            content_id: id.to_string(),
            watch_time_seconds: watch_time,
            view_count: views,
            unique_viewers: viewers,
        }
    }

    #[test]
    fn test_jsd_identical_distributions() {
        let p = vec![0.25, 0.25, 0.25, 0.25];
        let q = vec![0.25, 0.25, 0.25, 0.25];
        let jsd = jensen_shannon_divergence(&p, &q);
        assert!((jsd - 0.0).abs() < 1e-10, "JSD of identical = 0, got {jsd}");
    }

    #[test]
    fn test_jsd_disjoint_distributions() {
        let p = vec![1.0, 0.0];
        let q = vec![0.0, 1.0];
        let jsd = jensen_shannon_divergence(&p, &q);
        let expected = 2.0_f64.ln();
        assert!(
            (jsd - expected).abs() < 1e-10,
            "JSD of disjoint = ln(2), got {jsd}"
        );
    }

    #[test]
    fn test_jsd_asymmetric() {
        let p = vec![0.9, 0.1];
        let q = vec![0.1, 0.9];
        let jsd = jensen_shannon_divergence(&p, &q);
        assert!(jsd > 0.0, "JSD should be positive");
        assert!(jsd < 2.0_f64.ln(), "JSD should be < ln(2)");
    }

    #[test]
    fn test_gini_uniform() {
        let values = vec![10.0, 10.0, 10.0, 10.0];
        let g = gini_coefficient(&values);
        assert!((g - 0.0).abs() < 1e-10, "Gini of uniform = 0, got {g}");
    }

    #[test]
    fn test_gini_maximal() {
        // One person has everything, rest have zero.
        let mut values = vec![0.0; 99];
        values.push(100.0);
        let g = gini_coefficient(&values);
        // For n items with one non-zero: G = (n-1)/n = 0.99
        assert!(
            (g - 0.99).abs() < 0.01,
            "Gini of max concentration ≈ 0.99, got {g}"
        );
    }

    #[test]
    fn test_jaccard_identical() {
        let items: Vec<ContentConsumption> = (0..10)
            .map(|i| make_content(&format!("c{i}"), 100.0 - i as f64, 10, 5))
            .collect();
        let j = jaccard_similarity_top_k(&items, &items, 5);
        assert!((j - 1.0).abs() < 1e-10, "Jaccard of identical = 1, got {j}");
    }

    #[test]
    fn test_jaccard_disjoint() {
        let t: Vec<ContentConsumption> = (0..5)
            .map(|i| make_content(&format!("t{i}"), 100.0, 10, 5))
            .collect();
        let c: Vec<ContentConsumption> = (0..5)
            .map(|i| make_content(&format!("c{i}"), 100.0, 10, 5))
            .collect();
        let j = jaccard_similarity_top_k(&t, &c, 5);
        assert!((j - 0.0).abs() < 1e-10, "Jaccard of disjoint = 0, got {j}");
    }

    #[test]
    fn test_analyze_no_interference() {
        let items: Vec<ContentConsumption> = (0..10)
            .map(|i| make_content(&format!("c{i}"), 100.0 + i as f64, 50, 25))
            .collect();
        let input = InterferenceInput {
            treatment: items.clone(),
            control: items,
            total_treatment_viewers: 1000,
            total_control_viewers: 1000,
        };
        let result = analyze_interference(&input, 0.05, 0.05).unwrap();
        assert!(!result.interference_detected);
        assert!(result.jensen_shannon_divergence < 1e-10);
        assert!((result.jaccard_similarity_top_100 - 1.0).abs() < 1e-10);
    }

    mod proptest_interference {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn jsd_in_range(
                p1 in 0.01f64..1.0,
                p2 in 0.01f64..1.0,
            ) {
                let p = vec![p1, 1.0 - p1];
                let q = vec![p2, 1.0 - p2];
                let jsd = jensen_shannon_divergence(&p, &q);
                prop_assert!(jsd >= -1e-15, "JSD negative: {jsd}");
                prop_assert!(jsd <= 2.0_f64.ln() + 1e-10, "JSD > ln(2): {jsd}");
            }

            #[test]
            fn gini_in_range(values in proptest::collection::vec(0.0f64..1000.0, 2..50)) {
                let g = gini_coefficient(&values);
                prop_assert!(g >= -1e-10, "Gini negative: {g}");
                prop_assert!(g <= 1.0 + 1e-10, "Gini > 1: {g}");
            }

            #[test]
            fn jaccard_in_range(n in 2usize..20) {
                let t: Vec<ContentConsumption> = (0..n)
                    .map(|i| make_content(&format!("c{i}"), (n - i) as f64, 10, 5))
                    .collect();
                let c: Vec<ContentConsumption> = (0..n)
                    .map(|i| make_content(&format!("c{}", i + n / 2), (n - i) as f64, 10, 5))
                    .collect();
                let j = jaccard_similarity_top_k(&t, &c, n);
                prop_assert!(j >= -1e-10, "Jaccard negative: {j}");
                prop_assert!(j <= 1.0 + 1e-10, "Jaccard > 1: {j}");
            }

            #[test]
            fn all_outputs_finite(
                n in 3usize..15,
                effect in 0.0f64..100.0,
            ) {
                let t: Vec<ContentConsumption> = (0..n)
                    .map(|i| make_content(&format!("c{i}"), 100.0 + effect + i as f64, 10, 5))
                    .collect();
                let c: Vec<ContentConsumption> = (0..n)
                    .map(|i| make_content(&format!("c{i}"), 100.0 + i as f64, 10, 5))
                    .collect();
                let input = InterferenceInput {
                    treatment: t,
                    control: c,
                    total_treatment_viewers: 1000,
                    total_control_viewers: 1000,
                };
                let result = analyze_interference(&input, 0.05, 0.05).unwrap();
                prop_assert!(result.jensen_shannon_divergence.is_finite());
                prop_assert!(result.jaccard_similarity_top_100.is_finite());
                prop_assert!(result.treatment_gini_coefficient.is_finite());
                prop_assert!(result.control_gini_coefficient.is_finite());
            }
        }
    }
}
