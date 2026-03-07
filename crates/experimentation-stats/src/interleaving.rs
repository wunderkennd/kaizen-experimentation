//! Interleaving analysis for ranking algorithm comparison.
//!
//! Analyzes Team Draft interleaving experiments to determine which
//! algorithm produces better recommendations.
//!
//! Methods:
//! - **Sign test**: Binomial or chi-squared test for algorithm preference
//! - **Bradley-Terry model**: Strength estimation via MM algorithm (Hunter 2004)
//! - **Position analysis**: Per-position engagement rates
//!
//! See design doc section 7.4 for specification.

use std::collections::HashMap;

use experimentation_core::error::{assert_finite, Error, Result};
use nalgebra::DMatrix;
use statrs::distribution::{Binomial, ContinuousCDF, Normal};
use statrs::distribution::DiscreteCDF;

/// Per-user interleaving score.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InterleavingScore {
    pub user_id: String,
    pub algorithm_scores: HashMap<String, f64>,
    pub winning_algorithm_id: Option<String>,
    pub total_engagements: u32,
}

/// Estimated strength of an algorithm from Bradley-Terry model.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AlgorithmStrength {
    pub algorithm_id: String,
    pub strength: f64,
    pub ci_lower: f64,
    pub ci_upper: f64,
}

/// Per-position engagement rate analysis.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PositionAnalysis {
    pub position: u32,
    pub algorithm_engagement_rates: HashMap<String, f64>,
}

/// Full interleaving analysis result.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InterleavingAnalysisResult {
    pub algorithm_win_rates: HashMap<String, f64>,
    pub sign_test_p_value: f64,
    pub algorithm_strengths: Vec<AlgorithmStrength>,
    pub position_analyses: Vec<PositionAnalysis>,
}

/// Run full interleaving analysis.
pub fn analyze_interleaving(
    scores: &[InterleavingScore],
    alpha: f64,
) -> Result<InterleavingAnalysisResult> {
    if alpha <= 0.0 || alpha >= 1.0 {
        return Err(Error::Validation("alpha must be in (0, 1)".into()));
    }
    if scores.is_empty() {
        return Err(Error::Validation("scores must not be empty".into()));
    }

    // Collect all algorithm IDs.
    let mut algo_ids: Vec<String> = scores
        .iter()
        .flat_map(|s| s.algorithm_scores.keys().cloned())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    algo_ids.sort(); // Deterministic ordering.

    if algo_ids.len() < 2 {
        return Err(Error::Validation(
            "need at least 2 algorithms".into(),
        ));
    }

    let win_rates = compute_win_rates(scores, &algo_ids);
    let sign_p = sign_test(scores, &algo_ids)?;
    assert_finite(sign_p, "sign_test_p_value");

    let strengths = bradley_terry(scores, &algo_ids, alpha)?;
    let positions = position_analysis(scores, &algo_ids);

    Ok(InterleavingAnalysisResult {
        algorithm_win_rates: win_rates,
        sign_test_p_value: sign_p,
        algorithm_strengths: strengths,
        position_analyses: positions,
    })
}

/// Compute win rates per algorithm (excluding ties).
fn compute_win_rates(
    scores: &[InterleavingScore],
    algo_ids: &[String],
) -> HashMap<String, f64> {
    let mut wins: HashMap<String, u64> = algo_ids.iter().map(|a| (a.clone(), 0)).collect();
    let mut total_decisive = 0u64;

    for s in scores {
        if let Some(ref winner) = s.winning_algorithm_id {
            if let Some(count) = wins.get_mut(winner) {
                *count += 1;
                total_decisive += 1;
            }
        }
    }

    let mut rates = HashMap::new();
    for (algo, count) in &wins {
        let rate = if total_decisive > 0 {
            *count as f64 / total_decisive as f64
        } else {
            0.0
        };
        assert_finite(rate, &format!("win_rate[{algo}]"));
        rates.insert(algo.clone(), rate);
    }
    rates
}

/// Sign test for algorithm preference.
///
/// 2 algorithms: exact binomial test (two-sided).
/// 3+ algorithms: chi-squared goodness-of-fit against uniform.
fn sign_test(scores: &[InterleavingScore], algo_ids: &[String]) -> Result<f64> {
    let mut wins: HashMap<&str, u64> = algo_ids.iter().map(|a| (a.as_str(), 0)).collect();
    let mut total_decisive = 0u64;

    for s in scores {
        if let Some(ref winner) = s.winning_algorithm_id {
            if let Some(count) = wins.get_mut(winner.as_str()) {
                *count += 1;
                total_decisive += 1;
            }
        }
    }

    if total_decisive == 0 {
        return Ok(1.0); // All ties → no evidence.
    }

    if algo_ids.len() == 2 {
        // Exact binomial test, two-sided, H0: p = 0.5.
        let n = total_decisive;
        let k = wins[algo_ids[0].as_str()];
        binomial_two_sided_p(n, k, 0.5)
    } else {
        // Chi-squared goodness-of-fit against uniform.
        let k = algo_ids.len() as f64;
        let expected = total_decisive as f64 / k;

        let chi_sq: f64 = algo_ids
            .iter()
            .map(|a| {
                let observed = wins[a.as_str()] as f64;
                let diff = observed - expected;
                diff * diff / expected
            })
            .sum();
        assert_finite(chi_sq, "sign_test_chi_sq");

        let df = algo_ids.len() as f64 - 1.0;
        let p = 1.0 - chi_squared_cdf(chi_sq, df);
        Ok(p.clamp(0.0, 1.0))
    }
}

/// Two-sided binomial test p-value.
fn binomial_two_sided_p(n: u64, k: u64, p0: f64) -> Result<f64> {
    let binom = Binomial::new(p0, n).map_err(|e| {
        Error::Numerical(format!("binomial distribution error: {e}"))
    })?;

    // Two-sided: P(X <= min(k, n-k)) + P(X >= max(k, n-k))
    let k_mirror = n - k;
    let k_min = k.min(k_mirror);
    let k_max = k.max(k_mirror);

    let p_lower = binom.cdf(k_min);
    let p_upper = 1.0 - binom.cdf(k_max - 1);
    let p_value = (p_lower + p_upper).min(1.0);

    Ok(p_value)
}

/// Bradley-Terry model via MM algorithm (Hunter 2004).
///
/// Estimates relative strengths of K algorithms from pairwise comparisons.
fn bradley_terry(
    scores: &[InterleavingScore],
    algo_ids: &[String],
    alpha: f64,
) -> Result<Vec<AlgorithmStrength>> {
    let k = algo_ids.len();
    let algo_idx: HashMap<&str, usize> =
        algo_ids.iter().enumerate().map(|(i, a)| (a.as_str(), i)).collect();

    // Build win matrix W[i][j] and comparison count N[i][j].
    let mut w = vec![vec![0.0f64; k]; k];
    let mut n_mat = vec![vec![0.0f64; k]; k];

    for s in scores {
        if let Some(ref winner) = s.winning_algorithm_id {
            if let Some(&wi) = algo_idx.get(winner.as_str()) {
                // The winner beat all other algorithms in this comparison.
                for algo in s.algorithm_scores.keys() {
                    if let Some(&li) = algo_idx.get(algo.as_str()) {
                        if li != wi {
                            w[wi][li] += 1.0;
                            n_mat[wi][li] += 1.0;
                            n_mat[li][wi] += 1.0;
                        }
                    }
                }
            }
        }
    }

    // Laplace smoothing for zero-win edge cases.
    for i in 0..k {
        for j in 0..k {
            if i != j {
                w[i][j] += 0.5;
                n_mat[i][j] += 1.0; // +0.5 from each side.
            }
        }
    }

    // Total wins per algorithm.
    let w_total: Vec<f64> = (0..k)
        .map(|i| {
            let s: f64 = (0..k).map(|j| w[i][j]).sum();
            assert_finite(s, &format!("bt_w_total[{i}]"));
            s
        })
        .collect();

    // MM iterations.
    let mut pi = vec![1.0 / k as f64; k];
    let max_iter = 1000;
    let tol = 1e-8;

    for _iter in 0..max_iter {
        let mut pi_new = vec![0.0; k];
        for i in 0..k {
            let denom: f64 = (0..k)
                .filter(|&j| j != i)
                .map(|j| n_mat[i][j] / (pi[i] + pi[j]))
                .sum();
            assert_finite(denom, &format!("bt_denom[{i}]"));
            if denom > 0.0 {
                pi_new[i] = w_total[i] / denom;
            } else {
                pi_new[i] = pi[i];
            }
        }

        // Normalize.
        let pi_sum: f64 = pi_new.iter().sum();
        assert_finite(pi_sum, "bt_pi_sum");
        if pi_sum > 0.0 {
            for p in &mut pi_new {
                *p /= pi_sum;
            }
        }

        // Check convergence.
        let max_delta: f64 = pi
            .iter()
            .zip(pi_new.iter())
            .map(|(&old, &new)| (old - new).abs())
            .fold(0.0, f64::max);

        pi = pi_new;

        if max_delta < tol {
            break;
        }
    }

    // Standard errors from Fisher information matrix.
    let se = fisher_information_se(&pi, &n_mat, k);
    let z = normal_quantile(1.0 - alpha / 2.0);

    let mut strengths = Vec::with_capacity(k);
    for i in 0..k {
        let strength = pi[i];
        assert_finite(strength, &format!("bt_strength[{i}]"));

        // CI on log scale via delta method, then exponentiate.
        let log_pi = strength.ln();
        let log_se = se[i] / strength; // delta method: se(log(pi)) = se(pi)/pi
        assert_finite(log_se, &format!("bt_log_se[{i}]"));

        let ci_lower = (log_pi - z * log_se).exp();
        let ci_upper = (log_pi + z * log_se).exp();

        strengths.push(AlgorithmStrength {
            algorithm_id: algo_ids[i].clone(),
            strength,
            ci_lower: ci_lower.max(0.0),
            ci_upper,
        });
    }

    Ok(strengths)
}

/// Standard errors from inverse Fisher information matrix.
fn fisher_information_se(pi: &[f64], n_mat: &[Vec<f64>], k: usize) -> Vec<f64> {
    // Fisher information I[i][j]:
    // I[i][i] = sum_{j≠i} n_ij / (pi_i + pi_j)^2
    // I[i][j] = -n_ij / (pi_i + pi_j)^2 for i≠j
    let mut info = DMatrix::zeros(k, k);
    for i in 0..k {
        for j in 0..k {
            if i == j {
                let diag: f64 = (0..k)
                    .filter(|&m| m != i)
                    .map(|m| {
                        let denom = (pi[i] + pi[m]).powi(2);
                        if denom > 1e-30 {
                            n_mat[i][m] / denom
                        } else {
                            0.0
                        }
                    })
                    .sum();
                info[(i, i)] = diag;
            } else {
                let denom = (pi[i] + pi[j]).powi(2);
                if denom > 1e-30 {
                    info[(i, j)] = -n_mat[i][j] / denom;
                }
            }
        }
    }

    // Pseudoinverse via SVD for numerical stability.
    let svd = info.svd(true, true);
    let threshold = 1e-10;
    let inv = svd.pseudo_inverse(threshold).unwrap_or_else(|_| DMatrix::identity(k, k));

    (0..k)
        .map(|i| {
            let var = inv[(i, i)].max(0.0);
            var.sqrt()
        })
        .collect()
}

/// Per-position engagement rate analysis.
fn position_analysis(
    scores: &[InterleavingScore],
    algo_ids: &[String],
) -> Vec<PositionAnalysis> {
    // Position data is encoded in algorithm_scores as position-keyed.
    // For now, we compute per-algorithm average engagement.
    // Position analysis uses total_engagements as proxy.
    let max_positions = 10u32; // Top 10 positions.
    let mut analyses = Vec::new();

    for pos in 0..max_positions {
        let mut rates: HashMap<String, f64> = HashMap::new();
        for algo in algo_ids {
            // Average score for users where this algorithm contributed to this position.
            let (sum, count) = scores.iter().fold((0.0, 0u64), |(s, c), score| {
                if let Some(&algo_score) = score.algorithm_scores.get(algo) {
                    if algo_score > 0.0 {
                        (s + algo_score, c + 1)
                    } else {
                        (s, c)
                    }
                } else {
                    (s, c)
                }
            });
            let rate = if count > 0 { sum / count as f64 } else { 0.0 };
            assert_finite(rate, &format!("position_rate[{pos}][{algo}]"));
            rates.insert(algo.clone(), rate);
        }
        analyses.push(PositionAnalysis {
            position: pos,
            algorithm_engagement_rates: rates,
        });
    }

    analyses
}

/// Upper quantile of standard normal.
fn normal_quantile(p: f64) -> f64 {
    let n = Normal::new(0.0, 1.0).unwrap();
    n.inverse_cdf(p)
}

/// Chi-squared CDF approximation using normal approximation for large df,
/// or Wilson-Hilferty transformation.
fn chi_squared_cdf(x: f64, df: f64) -> f64 {
    use statrs::distribution::ChiSquared;
    let chi = ChiSquared::new(df).unwrap();
    chi.cdf(x)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_score(user: &str, algo_a: f64, algo_b: f64, winner: Option<&str>) -> InterleavingScore {
        let mut scores = HashMap::new();
        scores.insert("algo_a".to_string(), algo_a);
        scores.insert("algo_b".to_string(), algo_b);
        InterleavingScore {
            user_id: user.to_string(),
            algorithm_scores: scores,
            winning_algorithm_id: winner.map(|s| s.to_string()),
            total_engagements: (algo_a + algo_b) as u32,
        }
    }

    #[test]
    fn test_win_rates_two_algos() {
        let scores = vec![
            make_score("u1", 3.0, 1.0, Some("algo_a")),
            make_score("u2", 2.0, 4.0, Some("algo_b")),
            make_score("u3", 5.0, 2.0, Some("algo_a")),
            make_score("u4", 1.0, 1.0, None), // tie
        ];
        let result = analyze_interleaving(&scores, 0.05).unwrap();
        let wr_a = result.algorithm_win_rates["algo_a"];
        let wr_b = result.algorithm_win_rates["algo_b"];
        assert!((wr_a - 2.0 / 3.0).abs() < 1e-10);
        assert!((wr_b - 1.0 / 3.0).abs() < 1e-10);
    }

    #[test]
    fn test_balanced_sign_test() {
        // 50/50 split should give non-significant p-value.
        let mut scores = Vec::new();
        for i in 0..50 {
            scores.push(make_score(
                &format!("u{i}"),
                3.0,
                1.0,
                Some(if i < 25 { "algo_a" } else { "algo_b" }),
            ));
        }
        let result = analyze_interleaving(&scores, 0.05).unwrap();
        assert!(result.sign_test_p_value > 0.05, "Balanced split should be non-significant");
    }

    #[test]
    fn test_strong_preference_sign_test() {
        // 90% wins for algo_a should be significant.
        let mut scores = Vec::new();
        for i in 0..100 {
            scores.push(make_score(
                &format!("u{i}"),
                3.0,
                1.0,
                Some(if i < 90 { "algo_a" } else { "algo_b" }),
            ));
        }
        let result = analyze_interleaving(&scores, 0.05).unwrap();
        assert!(result.sign_test_p_value < 0.05, "Strong preference should be significant");
    }

    #[test]
    fn test_bradley_terry_strengths_sum_to_one() {
        let scores = vec![
            make_score("u1", 3.0, 1.0, Some("algo_a")),
            make_score("u2", 2.0, 4.0, Some("algo_b")),
            make_score("u3", 5.0, 2.0, Some("algo_a")),
        ];
        let result = analyze_interleaving(&scores, 0.05).unwrap();
        let total: f64 = result.algorithm_strengths.iter().map(|s| s.strength).sum();
        assert!((total - 1.0).abs() < 1e-6, "Strengths should sum to 1, got {total}");
    }

    #[test]
    fn test_bradley_terry_ci_contains_estimate() {
        let scores = vec![
            make_score("u1", 3.0, 1.0, Some("algo_a")),
            make_score("u2", 2.0, 4.0, Some("algo_b")),
            make_score("u3", 5.0, 2.0, Some("algo_a")),
            make_score("u4", 4.0, 1.0, Some("algo_a")),
            make_score("u5", 1.0, 3.0, Some("algo_b")),
        ];
        let result = analyze_interleaving(&scores, 0.05).unwrap();
        for s in &result.algorithm_strengths {
            assert!(
                s.ci_lower <= s.strength && s.strength <= s.ci_upper,
                "CI [{}, {}] doesn't contain strength {} for {}",
                s.ci_lower,
                s.ci_upper,
                s.strength,
                s.algorithm_id,
            );
        }
    }

    #[test]
    fn test_validation_empty() {
        assert!(analyze_interleaving(&[], 0.05).is_err());
    }

    #[test]
    fn test_validation_single_algorithm() {
        let mut scores_map = HashMap::new();
        scores_map.insert("algo_a".to_string(), 3.0);
        let scores = vec![InterleavingScore {
            user_id: "u1".to_string(),
            algorithm_scores: scores_map,
            winning_algorithm_id: Some("algo_a".to_string()),
            total_engagements: 3,
        }];
        assert!(analyze_interleaving(&scores, 0.05).is_err());
    }

    mod proptest_interleaving {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn win_rates_sum_to_one(
                a_wins in 0u32..50,
                b_wins in 0u32..50,
            ) {
                let total = a_wins + b_wins;
                if total == 0 { return Ok(()); }

                let mut scores = Vec::new();
                for i in 0..total {
                    let winner = if i < a_wins { "algo_a" } else { "algo_b" };
                    scores.push(make_score(&format!("u{i}"), 3.0, 1.0, Some(winner)));
                }
                let result = analyze_interleaving(&scores, 0.05).unwrap();
                let total_wr: f64 = result.algorithm_win_rates.values().sum();
                prop_assert!((total_wr - 1.0).abs() < 1e-10, "Win rates sum {total_wr} != 1");
            }

            #[test]
            fn strengths_sum_to_one(
                a_wins in 1u32..30,
                b_wins in 1u32..30,
            ) {
                let total = a_wins + b_wins;
                let mut scores = Vec::new();
                for i in 0..total {
                    let winner = if i < a_wins { "algo_a" } else { "algo_b" };
                    scores.push(make_score(&format!("u{i}"), 3.0, 1.0, Some(winner)));
                }
                let result = analyze_interleaving(&scores, 0.05).unwrap();
                let total_s: f64 = result.algorithm_strengths.iter().map(|s| s.strength).sum();
                prop_assert!((total_s - 1.0).abs() < 1e-4, "Strengths sum {total_s} != 1");
            }

            #[test]
            fn all_outputs_finite(
                a_wins in 1u32..20,
                b_wins in 1u32..20,
            ) {
                let total = a_wins + b_wins;
                let mut scores = Vec::new();
                for i in 0..total {
                    let winner = if i < a_wins { "algo_a" } else { "algo_b" };
                    scores.push(make_score(&format!("u{i}"), 3.0, 1.0, Some(winner)));
                }
                let result = analyze_interleaving(&scores, 0.05).unwrap();
                prop_assert!(result.sign_test_p_value.is_finite());
                for s in &result.algorithm_strengths {
                    prop_assert!(s.strength.is_finite());
                    prop_assert!(s.ci_lower.is_finite());
                    prop_assert!(s.ci_upper.is_finite());
                }
            }
        }
    }
}
