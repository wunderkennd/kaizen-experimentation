//! Multiple comparison correction methods.
//!
//! Controls family-wise error rate (FWER) or false discovery rate (FDR)
//! when testing multiple hypotheses simultaneously.
//!
//! - **Bonferroni**: Controls FWER. Conservative. adjusted_p = min(p × n, 1).
//! - **Benjamini-Hochberg**: Controls FDR. More powerful than Bonferroni.
//!   Step-up procedure with monotonicity enforcement.
//!
//! Validated against R's `p.adjust()` to 6 decimal places.

use experimentation_core::error::{assert_finite, Error, Result};

/// Which correction method was used.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum CorrectionMethod {
    BenjaminiHochberg,
    Bonferroni,
}

/// Result of a multiple comparison correction.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MultipleComparisonResult {
    /// Original p-values (in input order).
    pub p_values_original: Vec<f64>,
    /// Adjusted p-values (in input order).
    pub p_values_adjusted: Vec<f64>,
    /// Whether each hypothesis is rejected at the given threshold.
    pub rejected: Vec<bool>,
    /// Which method was used.
    pub method: CorrectionMethod,
}

/// Benjamini-Hochberg FDR correction.
///
/// Matches R's `p.adjust(p, method="BH")`.
pub fn benjamini_hochberg(p_values: &[f64], fdr: f64) -> Result<MultipleComparisonResult> {
    validate_p_values(p_values)?;
    if fdr <= 0.0 || fdr >= 1.0 {
        return Err(Error::Validation("fdr must be in (0, 1)".into()));
    }

    let n = p_values.len();

    let mut sorted_indices: Vec<usize> = (0..n).collect();
    sorted_indices.sort_by(|&a, &b| p_values[a].partial_cmp(&p_values[b]).unwrap());

    let mut adjusted_sorted: Vec<f64> = sorted_indices
        .iter()
        .enumerate()
        .map(|(rank_0, &idx)| {
            let rank = rank_0 + 1;
            let adj = (p_values[idx] * n as f64 / rank as f64).min(1.0);
            assert_finite(adj, &format!("bh_adjusted[rank={rank}]"));
            adj
        })
        .collect();

    if n >= 2 {
        for i in (0..n - 1).rev() {
            adjusted_sorted[i] = adjusted_sorted[i].min(adjusted_sorted[i + 1]);
        }
    }

    let mut p_values_adjusted = vec![0.0; n];
    for (rank_0, &orig_idx) in sorted_indices.iter().enumerate() {
        p_values_adjusted[orig_idx] = adjusted_sorted[rank_0];
    }

    let rejected: Vec<bool> = p_values_adjusted.iter().map(|&p| p <= fdr).collect();

    Ok(MultipleComparisonResult {
        p_values_original: p_values.to_vec(),
        p_values_adjusted,
        rejected,
        method: CorrectionMethod::BenjaminiHochberg,
    })
}

/// Bonferroni FWER correction.
///
/// Matches R's `p.adjust(p, method="bonferroni")`.
pub fn bonferroni(p_values: &[f64], alpha: f64) -> Result<MultipleComparisonResult> {
    validate_p_values(p_values)?;
    if alpha <= 0.0 || alpha >= 1.0 {
        return Err(Error::Validation("alpha must be in (0, 1)".into()));
    }

    let n = p_values.len();
    let p_values_adjusted: Vec<f64> = p_values
        .iter()
        .enumerate()
        .map(|(i, &p)| {
            let adj = (p * n as f64).min(1.0);
            assert_finite(adj, &format!("bonferroni_adjusted[{i}]"));
            adj
        })
        .collect();

    let rejected: Vec<bool> = p_values_adjusted.iter().map(|&p| p <= alpha).collect();

    Ok(MultipleComparisonResult {
        p_values_original: p_values.to_vec(),
        p_values_adjusted,
        rejected,
        method: CorrectionMethod::Bonferroni,
    })
}

fn validate_p_values(p_values: &[f64]) -> Result<()> {
    if p_values.is_empty() {
        return Err(Error::Validation("p_values must not be empty".into()));
    }
    for (i, &p) in p_values.iter().enumerate() {
        assert_finite(p, &format!("p_value[{i}]"));
        if !(0.0..=1.0).contains(&p) {
            return Err(Error::Validation(format!(
                "p_value[{i}] = {p} is not in [0, 1]"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bonferroni_basic() {
        let p = [0.01, 0.04, 0.03];
        let result = bonferroni(&p, 0.05).unwrap();
        assert_eq!(result.p_values_adjusted, vec![0.03, 0.12, 0.09]);
        assert_eq!(result.rejected, vec![true, false, false]);
    }

    #[test]
    fn test_bonferroni_caps_at_one() {
        let p = [0.5, 0.8];
        let result = bonferroni(&p, 0.05).unwrap();
        assert_eq!(result.p_values_adjusted, vec![1.0, 1.0]);
    }

    #[test]
    fn test_bh_basic() {
        let p = [0.01, 0.04, 0.03];
        let result = benjamini_hochberg(&p, 0.05).unwrap();
        assert!((result.p_values_adjusted[0] - 0.03).abs() < 1e-10);
        assert!((result.p_values_adjusted[1] - 0.04).abs() < 1e-10);
        assert!((result.p_values_adjusted[2] - 0.04).abs() < 1e-10);
    }

    #[test]
    fn test_bh_monotonicity_enforcement() {
        let p = [0.04, 0.01, 0.03];
        let result = benjamini_hochberg(&p, 0.05).unwrap();
        assert!((result.p_values_adjusted[0] - 0.04).abs() < 1e-10);
        assert!((result.p_values_adjusted[1] - 0.03).abs() < 1e-10);
        assert!((result.p_values_adjusted[2] - 0.04).abs() < 1e-10);
    }

    #[test]
    fn test_single_pvalue() {
        let result = bonferroni(&[0.03], 0.05).unwrap();
        assert_eq!(result.p_values_adjusted, vec![0.03]);
        assert_eq!(result.rejected, vec![true]);

        let result = benjamini_hochberg(&[0.03], 0.05).unwrap();
        assert_eq!(result.p_values_adjusted, vec![0.03]);
        assert_eq!(result.rejected, vec![true]);
    }

    #[test]
    fn test_validation_empty() {
        assert!(bonferroni(&[], 0.05).is_err());
        assert!(benjamini_hochberg(&[], 0.05).is_err());
    }

    #[test]
    fn test_validation_invalid_pvalue() {
        assert!(bonferroni(&[0.5, -0.1], 0.05).is_err());
        assert!(bonferroni(&[0.5, 1.1], 0.05).is_err());
    }

    #[test]
    fn test_validation_invalid_alpha() {
        assert!(bonferroni(&[0.5], 0.0).is_err());
        assert!(bonferroni(&[0.5], 1.0).is_err());
        assert!(benjamini_hochberg(&[0.5], 0.0).is_err());
    }

    #[test]
    #[should_panic(expected = "FAIL-FAST")]
    fn test_nan_pvalue_panics() {
        let _ = bonferroni(&[0.5, f64::NAN], 0.05);
    }

    #[test]
    fn test_all_zero_pvalues() {
        let p = [0.0, 0.0, 0.0];
        let result = bonferroni(&p, 0.05).unwrap();
        assert_eq!(result.rejected, vec![true, true, true]);
        let result = benjamini_hochberg(&p, 0.05).unwrap();
        assert_eq!(result.rejected, vec![true, true, true]);
    }

    mod proptest_mcc {
        use super::*;
        use proptest::prelude::*;

        fn valid_pvalue() -> impl Strategy<Value = f64> {
            0.0f64..=1.0f64
        }

        fn valid_pvalues(min: usize, max: usize) -> impl Strategy<Value = Vec<f64>> {
            prop::collection::vec(valid_pvalue(), min..=max)
        }

        proptest! {
            #[test]
            fn bonferroni_adjusted_ge_original(p_values in valid_pvalues(1, 20)) {
                let result = bonferroni(&p_values, 0.05).unwrap();
                for (orig, adj) in p_values.iter().zip(result.p_values_adjusted.iter()) {
                    prop_assert!(*adj >= *orig - 1e-15,
                        "adjusted {} < original {}", adj, orig);
                }
            }

            #[test]
            fn bonferroni_exact_formula(p_values in valid_pvalues(1, 20)) {
                let n = p_values.len();
                let result = bonferroni(&p_values, 0.05).unwrap();
                for (i, &p) in p_values.iter().enumerate() {
                    let expected = (p * n as f64).min(1.0);
                    prop_assert!((result.p_values_adjusted[i] - expected).abs() < 1e-15);
                }
            }

            #[test]
            fn bh_adjusted_ge_original(p_values in valid_pvalues(1, 20)) {
                let result = benjamini_hochberg(&p_values, 0.05).unwrap();
                for (orig, adj) in p_values.iter().zip(result.p_values_adjusted.iter()) {
                    prop_assert!(*adj >= *orig - 1e-15,
                        "adjusted {} < original {}", adj, orig);
                }
            }

            #[test]
            fn adjusted_pvalues_in_unit_interval(p_values in valid_pvalues(1, 20)) {
                let result_bf = bonferroni(&p_values, 0.05).unwrap();
                for &p in &result_bf.p_values_adjusted {
                    prop_assert!(p >= 0.0 && p <= 1.0, "Bonferroni adj {} out of [0,1]", p);
                }
                let result_bh = benjamini_hochberg(&p_values, 0.05).unwrap();
                for &p in &result_bh.p_values_adjusted {
                    prop_assert!(p >= 0.0 && p <= 1.0, "BH adj {} out of [0,1]", p);
                }
            }

            #[test]
            fn bh_sorted_adjusted_monotonic(p_values in valid_pvalues(2, 20)) {
                let result = benjamini_hochberg(&p_values, 0.05).unwrap();
                let mut pairs: Vec<(f64, f64)> = p_values.iter()
                    .zip(result.p_values_adjusted.iter())
                    .map(|(&o, &a)| (o, a))
                    .collect();
                pairs.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
                for i in 1..pairs.len() {
                    prop_assert!(pairs[i].1 >= pairs[i-1].1 - 1e-15,
                        "BH monotonicity violated: adj[{}]={} > adj[{}]={}",
                        i-1, pairs[i-1].1, i, pairs[i].1);
                }
            }
        }
    }
}
