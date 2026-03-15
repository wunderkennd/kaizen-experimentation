//! Clustered standard errors (HC1 sandwich estimator).
//!
//! Corrects standard errors for within-cluster correlation in session-level
//! experiments where a single user contributes multiple observations.
//!
//! Validated against R: `sandwich::vcovCL(lm(y ~ treatment), cluster = ~user_id, type = "HC1")`.
//! Golden files in `tests/golden/`.

use experimentation_core::error::{assert_finite, Error, Result};
use std::collections::HashMap;

/// A single observation with its value, cluster membership, and treatment assignment.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ClusteredObservation {
    pub value: f64,
    /// Cluster identifier (typically user_id).
    pub cluster_id: String,
    /// True if this observation belongs to the treatment group.
    pub is_treatment: bool,
}

/// Result of clustered standard error computation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ClusteredSeResult {
    /// Naive (OLS) standard error of the treatment coefficient.
    pub naive_se: f64,
    /// HC1 cluster-robust standard error.
    pub clustered_se: f64,
    /// Design effect: clustered_var / naive_var.
    pub design_effect: f64,
    /// P-value using naive SE (t-distribution with N-2 df).
    pub naive_p_value: f64,
    /// P-value using clustered SE (t-distribution with G-1 df).
    pub clustered_p_value: f64,
}

/// Compute HC1 clustered standard errors for a two-group comparison.
///
/// # Algorithm
/// 1. Fit OLS: Y = beta_0 + beta_1 * treatment + epsilon.
/// 2. Compute naive SE from (X'X)^-1 * s^2.
/// 3. Group residuals by cluster_id.
/// 4. HC1 meat: sum_g (X_g'e_g)(X_g'e_g)' * G/(G-1) * N/(N-p).
/// 5. Clustered V = (X'X)^-1 * meat * (X'X)^-1.
/// 6. Design effect = clustered_var / naive_var.
/// 7. P-values from t-distribution.
pub fn clustered_se(
    observations: &[ClusteredObservation],
    alpha: f64,
) -> Result<ClusteredSeResult> {
    let _ = alpha; // alpha used for documentation of context; p-values are two-sided

    let n = observations.len();
    if n < 3 {
        return Err(Error::Validation(
            "need at least 3 observations for clustered SE".into(),
        ));
    }

    let has_treatment = observations.iter().any(|o| o.is_treatment);
    let has_control = observations.iter().any(|o| !o.is_treatment);
    if !has_treatment || !has_control {
        return Err(Error::Validation(
            "need both treatment and control observations".into(),
        ));
    }

    // Count clusters.
    let mut cluster_set: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, obs) in observations.iter().enumerate() {
        cluster_set.entry(&obs.cluster_id).or_default().push(i);
    }
    let g = cluster_set.len(); // number of clusters
    if g < 2 {
        return Err(Error::Validation(
            "need at least 2 clusters for clustered SE".into(),
        ));
    }

    let n_f = n as f64;
    let p = 2.0; // intercept + treatment indicator

    // Build X'X and X'Y for the 2x2 system.
    // X = [1, treatment_i], so:
    // X'X = [[n, n_t], [n_t, n_t]]
    // X'Y = [sum_y, sum_y_treated]
    let n_t: f64 = observations.iter().filter(|o| o.is_treatment).count() as f64;
    let n_c: f64 = n_f - n_t;

    let sum_y: f64 = observations.iter().map(|o| o.value).sum();
    let sum_y_t: f64 = observations
        .iter()
        .filter(|o| o.is_treatment)
        .map(|o| o.value)
        .sum();
    assert_finite(sum_y, "clustering sum_y");
    assert_finite(sum_y_t, "clustering sum_y_t");

    // (X'X) = [[n, n_t], [n_t, n_t]]
    // det = n * n_t - n_t * n_t = n_t * (n - n_t) = n_t * n_c
    let det = n_t * n_c;
    if det.abs() < 1e-15 {
        return Err(Error::Numerical(
            "singular X'X matrix (all observations in one group)".into(),
        ));
    }

    // (X'X)^-1 = (1/det) * [[n_t, -n_t], [-n_t, n]]
    let xtx_inv_00 = n_t / det;
    let xtx_inv_01 = -n_t / det;
    let xtx_inv_10 = -n_t / det;
    let xtx_inv_11 = n_f / det;
    assert_finite(xtx_inv_11, "clustering xtx_inv_11");

    // OLS coefficients: beta = (X'X)^-1 * X'Y
    let beta_0 = xtx_inv_00 * sum_y + xtx_inv_01 * sum_y_t;
    let beta_1 = xtx_inv_10 * sum_y + xtx_inv_11 * sum_y_t;
    assert_finite(beta_0, "clustering beta_0");
    assert_finite(beta_1, "clustering beta_1");

    // Residuals: e_i = y_i - beta_0 - beta_1 * x_i.
    let residuals: Vec<f64> = observations
        .iter()
        .map(|o| {
            let x = if o.is_treatment { 1.0 } else { 0.0 };
            let r = o.value - beta_0 - beta_1 * x;
            assert_finite(r, "clustering residual");
            r
        })
        .collect();

    // Naive SE: s^2 = sum(e_i^2) / (N - p), SE(beta_1) = sqrt(s^2 * (X'X)^-1[1,1]).
    let sse: f64 = residuals.iter().map(|e| e * e).sum();
    assert_finite(sse, "clustering sse");
    let s2 = sse / (n_f - p);
    assert_finite(s2, "clustering s2");

    let naive_var = s2 * xtx_inv_11;
    assert_finite(naive_var, "clustering naive_var");
    let naive_se = naive_var.sqrt();
    assert_finite(naive_se, "clustering naive_se");

    // HC1 meat: sum_g (X_g'e_g)(X_g'e_g)' with small-sample correction G/(G-1) * N/(N-p).
    // For each cluster g, compute s_g = X_g'e_g (a 2-vector).
    // meat[i,j] = sum_g s_g[i] * s_g[j].
    let g_f = g as f64;
    let correction = (g_f / (g_f - 1.0)) * (n_f / (n_f - p));
    assert_finite(correction, "clustering HC1 correction");

    let mut meat_00 = 0.0;
    let mut meat_01 = 0.0;
    let mut meat_11 = 0.0;

    for indices in cluster_set.values() {
        // s_g = X_g'e_g = [sum(e_i for i in g), sum(x_i*e_i for i in g)]
        let mut s0 = 0.0;
        let mut s1 = 0.0;
        for &i in indices {
            let x = if observations[i].is_treatment {
                1.0
            } else {
                0.0
            };
            s0 += residuals[i];
            s1 += x * residuals[i];
        }
        meat_00 += s0 * s0;
        meat_01 += s0 * s1;
        meat_11 += s1 * s1;
    }

    meat_00 *= correction;
    meat_01 *= correction;
    meat_11 *= correction;
    assert_finite(meat_11, "clustering meat_11");

    // Clustered V = (X'X)^-1 * meat * (X'X)^-1.
    // We only need V[1,1] (variance of beta_1).
    // V = A * meat * A where A = (X'X)^-1.
    // V[1,1] = sum_j sum_k A[1,j] * meat[j,k] * A[k,1]
    // For 2x2: V[1,1] = A10*M00*A01 + A10*M01*A11 + A11*M10*A01 + A11*M11*A11
    // Note: meat is symmetric so M01 = M10
    let clustered_var = xtx_inv_10 * meat_00 * xtx_inv_01
        + xtx_inv_10 * meat_01 * xtx_inv_11
        + xtx_inv_11 * meat_01 * xtx_inv_01
        + xtx_inv_11 * meat_11 * xtx_inv_11;
    assert_finite(clustered_var, "clustering clustered_var");

    // Ensure non-negative variance (can go very slightly negative due to floating point).
    let clustered_var = clustered_var.max(0.0);
    let clustered_se = clustered_var.sqrt();
    assert_finite(clustered_se, "clustering clustered_se");

    let design_effect = if naive_var > 1e-15 {
        clustered_var / naive_var
    } else {
        1.0
    };
    assert_finite(design_effect, "clustering design_effect");

    // P-values from t-distribution.
    use statrs::distribution::{ContinuousCDF, StudentsT};

    // Naive p-value: t(N-2).
    let naive_p_value = if naive_se > 1e-15 {
        let t_stat = beta_1 / naive_se;
        assert_finite(t_stat, "clustering naive t_stat");
        let df = n_f - p;
        let t_dist = StudentsT::new(0.0, 1.0, df).map_err(|e| Error::Numerical(format!("{e}")))?;
        2.0 * (1.0 - t_dist.cdf(t_stat.abs()))
    } else {
        if beta_1.abs() < 1e-15 {
            1.0
        } else {
            0.0
        }
    };
    assert_finite(naive_p_value, "clustering naive_p_value");

    // Clustered p-value: t(G-1).
    let clustered_p_value = if clustered_se > 1e-15 {
        let t_stat = beta_1 / clustered_se;
        assert_finite(t_stat, "clustering clustered t_stat");
        let df = g_f - 1.0;
        let t_dist = StudentsT::new(0.0, 1.0, df).map_err(|e| Error::Numerical(format!("{e}")))?;
        2.0 * (1.0 - t_dist.cdf(t_stat.abs()))
    } else {
        if beta_1.abs() < 1e-15 {
            1.0
        } else {
            0.0
        }
    };
    assert_finite(clustered_p_value, "clustering clustered_p_value");

    Ok(ClusteredSeResult {
        naive_se,
        clustered_se,
        design_effect,
        naive_p_value,
        clustered_p_value,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_obs(
        values: &[f64],
        cluster_ids: &[&str],
        treatments: &[bool],
    ) -> Vec<ClusteredObservation> {
        values
            .iter()
            .zip(cluster_ids.iter())
            .zip(treatments.iter())
            .map(|((&v, &c), &t)| ClusteredObservation {
                value: v,
                cluster_id: c.to_string(),
                is_treatment: t,
            })
            .collect()
    }

    #[test]
    fn test_no_clustering_design_effect_near_one() {
        // Each user has exactly 1 observation → design effect ≈ 1.
        let obs = make_obs(
            &[1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0],
            &["u1", "u2", "u3", "u4", "u5", "u6", "u7", "u8"],
            &[false, false, false, false, true, true, true, true],
        );
        let result = clustered_se(&obs, 0.05).unwrap();
        // With N=10 and HC1 correction G/(G-1) * N/(N-p), design effect is
        // slightly above 1.0 even with no clustering. 0.25 tolerance for small samples.
        assert!(
            (result.design_effect - 1.0).abs() < 0.25,
            "design_effect: expected ~1.0, got {}",
            result.design_effect
        );
    }

    #[test]
    fn test_high_clustering_increases_se() {
        // Multiple correlated observations per user.
        let mut obs = Vec::new();
        // Control: 2 users, 5 observations each, all similar within user.
        for i in 0..5 {
            obs.push(ClusteredObservation {
                value: 1.0 + i as f64 * 0.01,
                cluster_id: "u1".into(),
                is_treatment: false,
            });
        }
        for i in 0..5 {
            obs.push(ClusteredObservation {
                value: 2.0 + i as f64 * 0.01,
                cluster_id: "u2".into(),
                is_treatment: false,
            });
        }
        // Treatment: 2 users, 5 observations each.
        for i in 0..5 {
            obs.push(ClusteredObservation {
                value: 5.0 + i as f64 * 0.01,
                cluster_id: "u3".into(),
                is_treatment: true,
            });
        }
        for i in 0..5 {
            obs.push(ClusteredObservation {
                value: 6.0 + i as f64 * 0.01,
                cluster_id: "u4".into(),
                is_treatment: true,
            });
        }

        let result = clustered_se(&obs, 0.05).unwrap();
        assert!(
            result.clustered_se >= result.naive_se,
            "clustered_se ({}) should be >= naive_se ({})",
            result.clustered_se,
            result.naive_se
        );
        assert!(result.design_effect >= 1.0);
    }

    #[test]
    fn test_validation_errors() {
        // Too few observations.
        let obs = make_obs(&[1.0, 2.0], &["u1", "u2"], &[false, true]);
        assert!(clustered_se(&obs, 0.05).is_err());

        // Only treatment.
        let obs = make_obs(&[1.0, 2.0, 3.0], &["u1", "u2", "u3"], &[true, true, true]);
        assert!(clustered_se(&obs, 0.05).is_err());

        // Only one cluster.
        let obs = make_obs(&[1.0, 2.0, 3.0], &["u1", "u1", "u1"], &[false, false, true]);
        assert!(clustered_se(&obs, 0.05).is_err());
    }

    #[test]
    fn test_all_outputs_finite() {
        let obs = make_obs(
            &[1.0, 2.0, 3.0, 4.0, 5.0, 6.0],
            &["u1", "u1", "u2", "u2", "u3", "u3"],
            &[false, false, false, true, true, true],
        );
        let result = clustered_se(&obs, 0.05).unwrap();
        assert!(result.naive_se.is_finite());
        assert!(result.clustered_se.is_finite());
        assert!(result.design_effect.is_finite());
        assert!(result.naive_p_value.is_finite());
        assert!(result.clustered_p_value.is_finite());
        assert!(result.naive_p_value >= 0.0 && result.naive_p_value <= 1.0);
        assert!(result.clustered_p_value >= 0.0 && result.clustered_p_value <= 1.0);
    }
}
