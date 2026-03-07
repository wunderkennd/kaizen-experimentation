//! Novelty/primacy effect detection via exponential decay fitting.
//!
//! Fits the model `f(t; s, a, d) = s + a·exp(-t/d)` to daily treatment
//! effects using weighted Gauss-Newton least squares, where weights are
//! proportional to sqrt(sample_size).
//!
//! - **Novelty**: positive amplitude `a`, effect decays over time
//! - **Primacy**: negative amplitude `a`, effect grows over time
//! - **Stable**: amplitude CI includes zero
//!
//! See design doc section 7.4 for specification.

use experimentation_core::error::{assert_finite, Error, Result};
use nalgebra::{DMatrix, DVector};

/// Daily treatment effect measurement.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DailyEffect {
    pub day: u32,
    pub effect: f64,
    pub sample_size: u64,
}

/// Result of novelty analysis.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NoveltyAnalysisResult {
    pub novelty_detected: bool,
    pub raw_treatment_effect: f64,
    pub projected_steady_state_effect: f64,
    pub novelty_amplitude: f64,
    pub decay_constant_days: f64,
    pub is_stabilized: bool,
    pub days_until_projected_stability: f64,
    pub amplitude_ci_lower: f64,
    pub amplitude_ci_upper: f64,
    pub r_squared: f64,
    pub residual_std_error: f64,
}

/// Analyze daily treatment effects for novelty/primacy patterns.
///
/// Fits `f(t) = s + a·exp(-t/d)` via weighted Gauss-Newton.
/// Requires at least 7 data points.
pub fn analyze_novelty(
    daily_effects: &[DailyEffect],
    alpha: f64,
) -> Result<NoveltyAnalysisResult> {
    if alpha <= 0.0 || alpha >= 1.0 {
        return Err(Error::Validation("alpha must be in (0, 1)".into()));
    }
    if daily_effects.len() < 7 {
        return Err(Error::Validation(
            "need at least 7 daily data points".into(),
        ));
    }

    // Validate inputs.
    for (i, de) in daily_effects.iter().enumerate() {
        assert_finite(de.effect, &format!("daily_effect[{i}]"));
        if de.sample_size == 0 {
            return Err(Error::Validation(format!(
                "sample_size[{i}] must be positive"
            )));
        }
    }

    let n = daily_effects.len();
    let t: Vec<f64> = daily_effects.iter().map(|de| de.day as f64).collect();
    let y: Vec<f64> = daily_effects.iter().map(|de| de.effect).collect();
    let w: Vec<f64> = daily_effects
        .iter()
        .map(|de| (de.sample_size as f64).sqrt())
        .collect();

    let raw_effect = y.iter().sum::<f64>() / n as f64;
    assert_finite(raw_effect, "raw_treatment_effect");

    // Grid search for best initial d: for fixed d, solve linear regression
    // for (s, a) since f(t) = s + a·exp(-t/d) is linear in (s, a).
    let mut params = best_initial_params(&t, &y, &w);

    // Levenberg-Marquardt iterations.
    let max_iter = 100;
    let conv_tol = 1e-8;
    let mut lambda = 1e-3; // LM damping parameter
    let mut converged = false;

    // Current cost for step acceptance.
    let (_, res_cur) = build_jacobian_and_residuals(&t, &y, &w, &params);
    let mut cost = res_cur.dot(&res_cur);

    for _iter in 0..max_iter {
        let (jac, residuals) = build_jacobian_and_residuals(&t, &y, &w, &params);

        // Damped normal equations: (J^T J + lambda·diag(J^T J)) dp = J^T r
        let jtj = jac.transpose() * &jac;
        let jtr = jac.transpose() * &residuals;

        let mut jtj_damped = jtj.clone();
        for i in 0..3 {
            jtj_damped[(i, i)] += lambda * jtj[(i, i)].max(1e-10);
        }

        let svd = jtj_damped.svd(true, true);
        let dp = match svd.solve(&jtr, 1e-12) {
            Ok(v) => v,
            Err(_) => {
                lambda *= 10.0;
                continue;
            }
        };

        // Trial step.
        let mut params_new = &params - &dp;
        params_new[2] = params_new[2].clamp(0.1, 365.0);

        let (_, res_new) = build_jacobian_and_residuals(&t, &y, &w, &params_new);
        let cost_new = res_new.dot(&res_new);

        if cost_new < cost {
            // Accept step, decrease damping.
            params = params_new;
            cost = cost_new;
            lambda = (lambda / 10.0).max(1e-10);
        } else {
            // Reject step, increase damping.
            lambda *= 10.0;
            if lambda > 1e10 {
                break; // Too much damping, give up.
            }
            continue;
        }

        // Check convergence: relative parameter change.
        let param_norm = params.norm();
        if param_norm > 0.0 && dp.norm() / param_norm < conv_tol {
            converged = true;
            break;
        }
    }

    let _ = converged; // Non-convergence is reflected in r_squared.

    let s = params[0]; // steady-state effect
    let a = params[1]; // amplitude
    let d = params[2]; // decay constant (days)

    assert_finite(s, "steady_state_effect");
    assert_finite(a, "novelty_amplitude");
    assert_finite(d, "decay_constant");

    // Compute standard errors from (J^T W J)^{-1} diagonal.
    let (jac_final, residuals_final) =
        build_jacobian_and_residuals(&t, &y, &w, &params);

    let dof = n as f64 - 3.0;
    let sse: f64 = residuals_final.iter().map(|r| r * r).sum();
    assert_finite(sse, "sse");

    let residual_std_error = if dof > 0.0 {
        (sse / dof).sqrt()
    } else {
        0.0
    };
    assert_finite(residual_std_error, "residual_std_error");

    // R-squared.
    let y_mean = y.iter().sum::<f64>() / n as f64;
    let ss_tot: f64 = y
        .iter()
        .zip(w.iter())
        .map(|(&yi, &wi)| {
            let r = wi * (yi - y_mean);
            r * r
        })
        .sum();
    let r_squared = if ss_tot > 0.0 {
        1.0 - sse / ss_tot
    } else {
        0.0
    };
    assert_finite(r_squared, "r_squared");

    // Standard errors from covariance matrix.
    let jtj = jac_final.transpose() * &jac_final;
    let svd = jtj.svd(true, true);
    let cov = svd
        .pseudo_inverse(1e-12)
        .unwrap_or_else(|_| DMatrix::identity(3, 3));
    let sigma_sq = if dof > 0.0 { sse / dof } else { 1.0 };

    let se_a = (cov[(1, 1)].max(0.0) * sigma_sq).sqrt();
    assert_finite(se_a, "se_amplitude");

    // Confidence interval for amplitude.
    let z = normal_quantile(1.0 - alpha / 2.0);
    let ci_lower = a - z * se_a;
    let ci_upper = a + z * se_a;

    // Detection logic.
    let amplitude_ci_excludes_zero = (ci_lower > 0.0) || (ci_upper < 0.0);
    let novelty_detected = amplitude_ci_excludes_zero && d < 14.0;

    // Stabilization check: last 7 days within 10% of steady state.
    let last_7 = &daily_effects[n.saturating_sub(7)..];
    let is_stabilized = last_7.iter().all(|de| {
        if s.abs() < 1e-10 {
            (de.effect - s).abs() < 0.1
        } else {
            ((de.effect - s) / s).abs() < 0.1
        }
    });

    // Days until stability: -d·ln(0.1·|s| / |a|).
    let days_until = if a.abs() > 1e-10 && s.abs() > 1e-10 {
        let ratio = 0.1 * s.abs() / a.abs();
        if ratio > 0.0 && ratio < 1.0 {
            -d * ratio.ln()
        } else {
            0.0
        }
    } else {
        0.0
    };
    assert_finite(days_until, "days_until_stability");

    Ok(NoveltyAnalysisResult {
        novelty_detected,
        raw_treatment_effect: raw_effect,
        projected_steady_state_effect: s,
        novelty_amplitude: a,
        decay_constant_days: d,
        is_stabilized,
        days_until_projected_stability: days_until.max(0.0),
        amplitude_ci_lower: ci_lower,
        amplitude_ci_upper: ci_upper,
        r_squared,
        residual_std_error,
    })
}

/// Grid search for initial parameters.
///
/// For each candidate d, solve the linear least squares problem for (s, a)
/// since f(t) = s + a·exp(-t/d) is linear in (s, a) for fixed d.
/// Pick the (s, a, d) with lowest weighted SSE.
fn best_initial_params(t: &[f64], y: &[f64], w: &[f64]) -> DVector<f64> {
    let d_candidates = [
        0.5, 1.0, 1.5, 2.0, 2.5, 3.0, 3.5, 4.0, 5.0, 6.0, 7.0, 8.0, 10.0,
        12.0, 14.0, 18.0, 21.0, 28.0, 42.0, 60.0,
    ];
    let mut best_cost = f64::MAX;
    let mut best_params = DVector::from_vec(vec![y[0], 0.0, 7.0]);

    for &d in &d_candidates {
        // Weighted linear regression: y = s + a·exp(-t/d)
        // Design matrix: [w_i, w_i·exp(-t_i/d)]
        let n = t.len();
        let mut ata = nalgebra::Matrix2::<f64>::zeros();
        let mut atb = nalgebra::Vector2::<f64>::zeros();

        for i in 0..n {
            let e = (-t[i] / d).exp();
            let x0 = w[i];
            let x1 = w[i] * e;
            let b = w[i] * y[i];

            ata[(0, 0)] += x0 * x0;
            ata[(0, 1)] += x0 * x1;
            ata[(1, 0)] += x0 * x1;
            ata[(1, 1)] += x1 * x1;
            atb[0] += x0 * b;
            atb[1] += x1 * b;
        }

        let det: f64 = ata[(0, 0)] * ata[(1, 1)] - ata[(0, 1)] * ata[(1, 0)];
        if det.abs() < 1e-30 {
            continue;
        }
        let s = (ata[(1, 1)] * atb[0] - ata[(0, 1)] * atb[1]) / det;
        let a = (ata[(0, 0)] * atb[1] - ata[(1, 0)] * atb[0]) / det;

        // Compute cost.
        let cost: f64 = (0..n)
            .map(|i| {
                let r = w[i] * (y[i] - s - a * (-t[i] / d).exp());
                r * r
            })
            .sum();

        if cost < best_cost {
            best_cost = cost;
            best_params = DVector::from_vec(vec![s, a, d]);
        }
    }

    best_params
}

/// Build Jacobian matrix and weighted residual vector.
///
/// Model: f(t; s, a, d) = s + a·exp(-t/d)
/// Jacobian rows: [∂f/∂s, ∂f/∂a, ∂f/∂d] = [1, exp(-t/d), a·t·exp(-t/d)/d²]
/// Residual: w_i · (y_i - f(t_i))
fn build_jacobian_and_residuals(
    t: &[f64],
    y: &[f64],
    w: &[f64],
    params: &DVector<f64>,
) -> (DMatrix<f64>, DVector<f64>) {
    let n = t.len();
    let s = params[0];
    let a = params[1];
    let d = params[2];

    let mut jac = DMatrix::zeros(n, 3);
    let mut res = DVector::zeros(n);

    for i in 0..n {
        let exp_val = (-t[i] / d).exp();
        assert_finite(exp_val, &format!("exp_val[{i}]"));

        let f_val = s + a * exp_val;
        assert_finite(f_val, &format!("f_val[{i}]"));

        // Weighted Jacobian.
        jac[(i, 0)] = w[i]; // ∂f/∂s = 1
        jac[(i, 1)] = w[i] * exp_val; // ∂f/∂a = exp(-t/d)
        jac[(i, 2)] = w[i] * a * t[i] * exp_val / (d * d); // ∂f/∂d = a·t·exp(-t/d)/d²

        // Weighted residual.
        res[i] = w[i] * (y[i] - f_val);
    }

    (jac, res)
}

/// Upper quantile of standard normal.
fn normal_quantile(p: f64) -> f64 {
    use statrs::distribution::{ContinuousCDF, Normal};
    let n = Normal::new(0.0, 1.0).unwrap();
    n.inverse_cdf(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_decay_data(s: f64, a: f64, d: f64, days: u32, noise: f64) -> Vec<DailyEffect> {
        use rand::SeedableRng;
        use rand_distr::{Distribution, Normal};
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);
        let normal = Normal::new(0.0, noise).unwrap();

        (0..days)
            .map(|day| {
                let effect = s + a * (-(day as f64) / d).exp() + normal.sample(&mut rng);
                DailyEffect {
                    day,
                    effect,
                    sample_size: 1000,
                }
            })
            .collect()
    }

    #[test]
    fn test_clear_novelty_decay() {
        let data = make_decay_data(5.0, 3.0, 4.0, 30, 0.1);
        let result = analyze_novelty(&data, 0.05).unwrap();
        // Parameters should be close to true values.
        assert!(
            (result.projected_steady_state_effect - 5.0).abs() < 1.0,
            "steady state: {}",
            result.projected_steady_state_effect
        );
        assert!(
            (result.novelty_amplitude - 3.0).abs() < 1.0,
            "amplitude: {}",
            result.novelty_amplitude
        );
        assert!(result.novelty_detected, "should detect novelty");
    }

    #[test]
    fn test_no_novelty_flat() {
        let data: Vec<DailyEffect> = (0..30)
            .map(|day| DailyEffect {
                day,
                effect: 2.0 + 0.01 * (day as f64 % 3.0 - 1.0),
                sample_size: 1000,
            })
            .collect();
        let result = analyze_novelty(&data, 0.05).unwrap();
        // Amplitude should be near zero → novelty not detected.
        assert!(
            !result.novelty_detected || result.novelty_amplitude.abs() < 0.5,
            "flat data should not detect novelty: amp={}",
            result.novelty_amplitude
        );
    }

    #[test]
    fn test_primacy_effect() {
        let data = make_decay_data(5.0, -2.0, 6.0, 30, 0.1);
        let result = analyze_novelty(&data, 0.05).unwrap();
        assert!(
            result.novelty_amplitude < 0.0,
            "primacy should have negative amplitude: {}",
            result.novelty_amplitude
        );
    }

    #[test]
    fn test_minimum_data_points() {
        let data: Vec<DailyEffect> = (0..6)
            .map(|day| DailyEffect {
                day,
                effect: 1.0,
                sample_size: 100,
            })
            .collect();
        assert!(analyze_novelty(&data, 0.05).is_err());
    }

    #[test]
    fn test_r_squared_reasonable() {
        let data = make_decay_data(5.0, 3.0, 4.0, 30, 0.1);
        let result = analyze_novelty(&data, 0.05).unwrap();
        assert!(
            result.r_squared > 0.5,
            "R² should be high for clean data: {}",
            result.r_squared
        );
    }

    #[test]
    fn test_ci_contains_amplitude() {
        let data = make_decay_data(5.0, 3.0, 4.0, 30, 0.1);
        let result = analyze_novelty(&data, 0.05).unwrap();
        assert!(
            result.amplitude_ci_lower <= result.novelty_amplitude
                && result.novelty_amplitude <= result.amplitude_ci_upper,
            "CI [{}, {}] should contain amplitude {}",
            result.amplitude_ci_lower,
            result.amplitude_ci_upper,
            result.novelty_amplitude,
        );
    }

    mod proptest_novelty {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn all_outputs_finite(
                s in -10.0f64..10.0,
                a in -5.0f64..5.0,
            ) {
                let data: Vec<DailyEffect> = (0..15)
                    .map(|day| DailyEffect {
                        day,
                        effect: s + a * (-(day as f64) / 5.0).exp(),
                        sample_size: 100,
                    })
                    .collect();
                let result = analyze_novelty(&data, 0.05).unwrap();
                prop_assert!(result.projected_steady_state_effect.is_finite());
                prop_assert!(result.novelty_amplitude.is_finite());
                prop_assert!(result.decay_constant_days.is_finite());
                prop_assert!(result.r_squared.is_finite());
                prop_assert!(result.residual_std_error.is_finite());
            }

            #[test]
            fn r_squared_in_unit_interval(
                s in 1.0f64..10.0,
                a in 0.5f64..5.0,
                d in 2.0f64..20.0,
            ) {
                let data: Vec<DailyEffect> = (0..20)
                    .map(|day| DailyEffect {
                        day,
                        effect: s + a * (-(day as f64) / d).exp(),
                        sample_size: 100,
                    })
                    .collect();
                let result = analyze_novelty(&data, 0.05).unwrap();
                // R² should be very close to 1 for noiseless data.
                prop_assert!(result.r_squared > 0.9, "R² should be > 0.9 for noiseless data: {}", result.r_squared);
            }
        }
    }
}
