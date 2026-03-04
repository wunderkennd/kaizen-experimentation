//! Sample Ratio Mismatch (SRM) detection.
//!
//! SRM occurs when the observed traffic split differs significantly from
//! the configured split. This is a critical diagnostic — SRM invalidates
//! all downstream analysis.

use experimentation_core::error::{assert_finite, Error, Result};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct SrmResult {
    pub chi_squared: f64,
    pub p_value: f64,
    pub is_mismatch: bool,
    pub observed: HashMap<String, u64>,
    pub expected: HashMap<String, f64>,
}

/// Run a chi-squared goodness-of-fit test for sample ratio mismatch.
///
/// # Arguments
/// * `observed` - Map of variant_id → observed count
/// * `expected_fractions` - Map of variant_id → expected traffic fraction (must sum to ~1.0)
/// * `alpha` - Significance threshold (typically 0.001 for SRM)
pub fn srm_check(
    observed: &HashMap<String, u64>,
    expected_fractions: &HashMap<String, f64>,
    alpha: f64,
) -> Result<SrmResult> {
    if observed.len() < 2 {
        return Err(Error::Validation("SRM requires at least 2 variants".into()));
    }

    let total: u64 = observed.values().sum();
    if total == 0 {
        return Err(Error::Validation("total observed count is 0".into()));
    }

    let total_f = total as f64;
    let mut chi_sq = 0.0;
    let df = (observed.len() - 1) as f64;

    for (variant_id, &obs_count) in observed {
        let fraction = expected_fractions
            .get(variant_id)
            .ok_or_else(|| Error::Validation(format!("missing expected fraction for {variant_id}")))?;

        let expected_count = fraction * total_f;
        assert_finite(expected_count, &format!("expected count for {variant_id}"));

        if expected_count == 0.0 {
            return Err(Error::Validation(format!("expected count is 0 for {variant_id}")));
        }

        let diff = obs_count as f64 - expected_count;
        chi_sq += (diff * diff) / expected_count;
    }

    assert_finite(chi_sq, "chi-squared statistic");

    // Chi-squared p-value using the regularized incomplete gamma function
    let p_value = 1.0 - statrs::distribution::ChiSquared::new(df)
        .map_err(|e| Error::Numerical(format!("chi-squared distribution error: {e}")))?
        .cdf(chi_sq);

    use statrs::distribution::ContinuousCDF;

    Ok(SrmResult {
        chi_squared: chi_sq,
        p_value,
        is_mismatch: p_value < alpha,
        observed: observed.clone(),
        expected: expected_fractions.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_mismatch() {
        let observed: HashMap<String, u64> =
            [("control".into(), 5000), ("treatment".into(), 5000)].into();
        let expected: HashMap<String, f64> =
            [("control".into(), 0.5), ("treatment".into(), 0.5)].into();

        let result = srm_check(&observed, &expected, 0.001).unwrap();
        assert!(!result.is_mismatch, "equal split should not trigger SRM");
    }

    #[test]
    fn test_clear_mismatch() {
        let observed: HashMap<String, u64> =
            [("control".into(), 6000), ("treatment".into(), 4000)].into();
        let expected: HashMap<String, f64> =
            [("control".into(), 0.5), ("treatment".into(), 0.5)].into();

        let result = srm_check(&observed, &expected, 0.001).unwrap();
        assert!(result.is_mismatch, "60/40 split on 50/50 expected should trigger SRM");
    }
}
