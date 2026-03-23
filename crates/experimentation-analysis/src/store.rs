//! PostgreSQL cache for analysis results.
//!
//! Stores computed results in PostgreSQL so that `GetAnalysisResult` can serve
//! from cache instead of recomputing from Delta Lake on every call.
//! Cache writes are fire-and-forget — errors are logged but never fail the RPC.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sqlx::postgres::{PgPool, PgPoolOptions};
use uuid::Uuid;

use experimentation_proto::experimentation::analysis::v1::{
    AnalysisResult, InterferenceAnalysisResult, IpwResult as ProtoIpwResult, MetricResult,
    NoveltyAnalysisResult, SegmentResult, SequentialResult, SessionLevelResult,
    SrmResult as ProtoSrmResult,
};

// ---------------------------------------------------------------------------
// CachedAnalysisResult: serde wrapper for the proto AnalysisResult
// ---------------------------------------------------------------------------

/// Mirrors `AnalysisResult` proto for JSON (de)serialization into `result_data` JSONB.
/// We avoid adding serde derives to the proto crate by using a local mirror type.
#[derive(Debug, Serialize, Deserialize)]
pub struct CachedAnalysisResult {
    pub experiment_id: String,
    pub metric_results: Vec<CachedMetricResult>,
    pub srm_result: Option<CachedSrmResult>,
    pub cochran_q_p_value: f64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CachedMetricResult {
    pub metric_id: String,
    pub variant_id: String,
    pub control_mean: f64,
    pub treatment_mean: f64,
    pub absolute_effect: f64,
    pub relative_effect: f64,
    pub ci_lower: f64,
    pub ci_upper: f64,
    pub p_value: f64,
    pub is_significant: bool,
    pub cuped_adjusted_effect: f64,
    pub cuped_ci_lower: f64,
    pub cuped_ci_upper: f64,
    pub variance_reduction_pct: f64,
    pub sequential_result: Option<CachedSequentialResult>,
    pub segment_results: Vec<CachedSegmentResult>,
    pub session_level_result: Option<CachedSessionLevelResult>,
    #[serde(default)]
    pub ipw_result: Option<CachedIpwResult>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CachedSequentialResult {
    pub boundary_crossed: bool,
    pub alpha_spent: f64,
    pub alpha_remaining: f64,
    pub current_look: i32,
    pub adjusted_p_value: f64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CachedSegmentResult {
    pub segment: i32,
    pub effect: f64,
    pub ci_lower: f64,
    pub ci_upper: f64,
    pub p_value: f64,
    pub sample_size: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CachedSessionLevelResult {
    pub naive_se: f64,
    pub clustered_se: f64,
    pub design_effect: f64,
    pub naive_p_value: f64,
    pub clustered_p_value: f64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CachedIpwResult {
    pub effect: f64,
    pub se: f64,
    pub ci_lower: f64,
    pub ci_upper: f64,
    pub p_value: f64,
    pub n_clipped: i32,
    pub effective_sample_size: f64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CachedSrmResult {
    pub chi_squared: f64,
    pub p_value: f64,
    pub is_mismatch: bool,
    pub observed_counts: std::collections::HashMap<String, i64>,
    pub expected_counts: std::collections::HashMap<String, i64>,
}

// ---------------------------------------------------------------------------
// Proto <-> Cached conversions
// ---------------------------------------------------------------------------

impl From<&AnalysisResult> for CachedAnalysisResult {
    fn from(r: &AnalysisResult) -> Self {
        Self {
            experiment_id: r.experiment_id.clone(),
            metric_results: r
                .metric_results
                .iter()
                .map(CachedMetricResult::from)
                .collect(),
            srm_result: r.srm_result.as_ref().map(CachedSrmResult::from),
            cochran_q_p_value: r.cochran_q_p_value,
        }
    }
}

impl From<&CachedAnalysisResult> for AnalysisResult {
    fn from(c: &CachedAnalysisResult) -> Self {
        Self {
            experiment_id: c.experiment_id.clone(),
            metric_results: c.metric_results.iter().map(MetricResult::from).collect(),
            srm_result: c.srm_result.as_ref().map(ProtoSrmResult::from),
            surrogate_projections: vec![],
            cochran_q_p_value: c.cochran_q_p_value,
            computed_at: Some(super::grpc::now_timestamp()),
        }
    }
}

impl From<&MetricResult> for CachedMetricResult {
    fn from(m: &MetricResult) -> Self {
        Self {
            metric_id: m.metric_id.clone(),
            variant_id: m.variant_id.clone(),
            control_mean: m.control_mean,
            treatment_mean: m.treatment_mean,
            absolute_effect: m.absolute_effect,
            relative_effect: m.relative_effect,
            ci_lower: m.ci_lower,
            ci_upper: m.ci_upper,
            p_value: m.p_value,
            is_significant: m.is_significant,
            cuped_adjusted_effect: m.cuped_adjusted_effect,
            cuped_ci_lower: m.cuped_ci_lower,
            cuped_ci_upper: m.cuped_ci_upper,
            variance_reduction_pct: m.variance_reduction_pct,
            sequential_result: m
                .sequential_result
                .as_ref()
                .map(CachedSequentialResult::from),
            segment_results: m
                .segment_results
                .iter()
                .map(CachedSegmentResult::from)
                .collect(),
            session_level_result: m
                .session_level_result
                .as_ref()
                .map(CachedSessionLevelResult::from),
            ipw_result: m.ipw_result.as_ref().map(CachedIpwResult::from),
        }
    }
}

impl From<&CachedMetricResult> for MetricResult {
    fn from(c: &CachedMetricResult) -> Self {
        Self {
            metric_id: c.metric_id.clone(),
            variant_id: c.variant_id.clone(),
            control_mean: c.control_mean,
            treatment_mean: c.treatment_mean,
            absolute_effect: c.absolute_effect,
            relative_effect: c.relative_effect,
            ci_lower: c.ci_lower,
            ci_upper: c.ci_upper,
            p_value: c.p_value,
            is_significant: c.is_significant,
            cuped_adjusted_effect: c.cuped_adjusted_effect,
            cuped_ci_lower: c.cuped_ci_lower,
            cuped_ci_upper: c.cuped_ci_upper,
            variance_reduction_pct: c.variance_reduction_pct,
            sequential_result: c.sequential_result.as_ref().map(SequentialResult::from),
            segment_results: c.segment_results.iter().map(SegmentResult::from).collect(),
            session_level_result: c
                .session_level_result
                .as_ref()
                .map(SessionLevelResult::from),
            ipw_result: c.ipw_result.as_ref().map(ProtoIpwResult::from),
            e_value: 0.0,
            log_e_value: 0.0,
        }
    }
}

impl From<&SequentialResult> for CachedSequentialResult {
    fn from(s: &SequentialResult) -> Self {
        Self {
            boundary_crossed: s.boundary_crossed,
            alpha_spent: s.alpha_spent,
            alpha_remaining: s.alpha_remaining,
            current_look: s.current_look,
            adjusted_p_value: s.adjusted_p_value,
        }
    }
}

impl From<&CachedSequentialResult> for SequentialResult {
    fn from(c: &CachedSequentialResult) -> Self {
        Self {
            boundary_crossed: c.boundary_crossed,
            alpha_spent: c.alpha_spent,
            alpha_remaining: c.alpha_remaining,
            current_look: c.current_look,
            adjusted_p_value: c.adjusted_p_value,
        }
    }
}

impl From<&SegmentResult> for CachedSegmentResult {
    fn from(s: &SegmentResult) -> Self {
        Self {
            segment: s.segment,
            effect: s.effect,
            ci_lower: s.ci_lower,
            ci_upper: s.ci_upper,
            p_value: s.p_value,
            sample_size: s.sample_size,
        }
    }
}

impl From<&CachedSegmentResult> for SegmentResult {
    fn from(c: &CachedSegmentResult) -> Self {
        Self {
            segment: c.segment,
            effect: c.effect,
            ci_lower: c.ci_lower,
            ci_upper: c.ci_upper,
            p_value: c.p_value,
            sample_size: c.sample_size,
        }
    }
}

impl From<&SessionLevelResult> for CachedSessionLevelResult {
    fn from(s: &SessionLevelResult) -> Self {
        Self {
            naive_se: s.naive_se,
            clustered_se: s.clustered_se,
            design_effect: s.design_effect,
            naive_p_value: s.naive_p_value,
            clustered_p_value: s.clustered_p_value,
        }
    }
}

impl From<&CachedSessionLevelResult> for SessionLevelResult {
    fn from(c: &CachedSessionLevelResult) -> Self {
        Self {
            naive_se: c.naive_se,
            clustered_se: c.clustered_se,
            design_effect: c.design_effect,
            naive_p_value: c.naive_p_value,
            clustered_p_value: c.clustered_p_value,
        }
    }
}

impl From<&ProtoIpwResult> for CachedIpwResult {
    fn from(i: &ProtoIpwResult) -> Self {
        Self {
            effect: i.effect,
            se: i.se,
            ci_lower: i.ci_lower,
            ci_upper: i.ci_upper,
            p_value: i.p_value,
            n_clipped: i.n_clipped,
            effective_sample_size: i.effective_sample_size,
        }
    }
}

impl From<&CachedIpwResult> for ProtoIpwResult {
    fn from(c: &CachedIpwResult) -> Self {
        Self {
            effect: c.effect,
            se: c.se,
            ci_lower: c.ci_lower,
            ci_upper: c.ci_upper,
            p_value: c.p_value,
            n_clipped: c.n_clipped,
            effective_sample_size: c.effective_sample_size,
        }
    }
}

impl From<&ProtoSrmResult> for CachedSrmResult {
    fn from(s: &ProtoSrmResult) -> Self {
        Self {
            chi_squared: s.chi_squared,
            p_value: s.p_value,
            is_mismatch: s.is_mismatch,
            observed_counts: s.observed_counts.clone(),
            expected_counts: s.expected_counts.clone(),
        }
    }
}

impl From<&CachedSrmResult> for ProtoSrmResult {
    fn from(c: &CachedSrmResult) -> Self {
        Self {
            chi_squared: c.chi_squared,
            p_value: c.p_value,
            is_mismatch: c.is_mismatch,
            observed_counts: c.observed_counts.clone(),
            expected_counts: c.expected_counts.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// AnalysisStore
// ---------------------------------------------------------------------------

type NoveltyRow = (
    String,
    bool,
    Option<f64>,
    Option<f64>,
    Option<f64>,
    Option<f64>,
    Option<bool>,
);
type InterferenceRow = (bool, Option<f64>, Option<f64>, Option<f64>, Option<f64>);

pub struct AnalysisStore {
    pool: PgPool,
}

impl AnalysisStore {
    /// Connect to PostgreSQL. Returns an error if the connection fails.
    pub async fn connect(database_url: &str) -> Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(database_url)
            .await
            .context("failed to connect to PostgreSQL")?;
        Ok(Self { pool })
    }

    // -- Analysis results ---------------------------------------------------

    /// Get the latest cached analysis result for an experiment.
    pub async fn get_analysis_result(
        &self,
        experiment_id: &Uuid,
    ) -> Result<Option<AnalysisResult>> {
        let row: Option<(serde_json::Value,)> = sqlx::query_as(
            "SELECT result_data FROM analysis_results \
             WHERE experiment_id = $1 \
             ORDER BY computed_at DESC LIMIT 1",
        )
        .bind(experiment_id)
        .fetch_optional(&self.pool)
        .await
        .context("failed to query analysis_results")?;

        match row {
            Some((json_val,)) => {
                let cached: CachedAnalysisResult = serde_json::from_value(json_val)
                    .context("failed to deserialize cached analysis result")?;
                Ok(Some(AnalysisResult::from(&cached)))
            }
            None => Ok(None),
        }
    }

    /// Save an analysis result to the cache.
    pub async fn save_analysis_result(
        &self,
        experiment_id: &Uuid,
        result: &AnalysisResult,
    ) -> Result<()> {
        let cached = CachedAnalysisResult::from(result);
        let json_val =
            serde_json::to_value(&cached).context("failed to serialize analysis result")?;

        let srm_p_value = result.srm_result.as_ref().map(|s| s.p_value);
        let srm_is_mismatch = result.srm_result.as_ref().map(|s| s.is_mismatch);
        let cochran_q = if result.cochran_q_p_value != 0.0 {
            Some(result.cochran_q_p_value)
        } else {
            None
        };

        sqlx::query(
            "INSERT INTO analysis_results \
             (experiment_id, result_data, srm_p_value, srm_is_mismatch, cochran_q_p_value) \
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(experiment_id)
        .bind(&json_val)
        .bind(srm_p_value)
        .bind(srm_is_mismatch)
        .bind(cochran_q)
        .execute(&self.pool)
        .await
        .context("failed to insert into analysis_results")?;

        Ok(())
    }

    // -- Novelty results ----------------------------------------------------

    /// Get the latest cached novelty analysis result for an experiment.
    #[allow(dead_code)]
    pub async fn get_novelty_result(
        &self,
        experiment_id: &Uuid,
    ) -> Result<Option<NoveltyAnalysisResult>> {
        let row: Option<NoveltyRow> = sqlx::query_as(
            "SELECT metric_id, novelty_detected, raw_treatment_effect, \
             projected_steady_state, novelty_amplitude, decay_constant_days, is_stabilized \
             FROM novelty_analysis_results \
             WHERE experiment_id = $1 \
             ORDER BY computed_at DESC LIMIT 1",
        )
        .bind(experiment_id)
        .fetch_optional(&self.pool)
        .await
        .context("failed to query novelty_analysis_results")?;

        match row {
            Some((
                metric_id,
                novelty_detected,
                raw_treatment_effect,
                projected_steady_state,
                novelty_amplitude,
                decay_constant_days,
                is_stabilized,
            )) => Ok(Some(NoveltyAnalysisResult {
                experiment_id: experiment_id.to_string(),
                metric_id,
                novelty_detected,
                raw_treatment_effect: raw_treatment_effect.unwrap_or(0.0),
                projected_steady_state_effect: projected_steady_state.unwrap_or(0.0),
                novelty_amplitude: novelty_amplitude.unwrap_or(0.0),
                decay_constant_days: decay_constant_days.unwrap_or(0.0),
                is_stabilized: is_stabilized.unwrap_or(false),
                days_until_projected_stability: 0.0,
                computed_at: Some(super::grpc::now_timestamp()),
            })),
            None => Ok(None),
        }
    }

    /// Save a novelty analysis result to the cache.
    pub async fn save_novelty_result(
        &self,
        experiment_id: &Uuid,
        result: &NoveltyAnalysisResult,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO novelty_analysis_results \
             (experiment_id, metric_id, novelty_detected, raw_treatment_effect, \
              projected_steady_state, novelty_amplitude, decay_constant_days, is_stabilized) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(experiment_id)
        .bind(&result.metric_id)
        .bind(result.novelty_detected)
        .bind(result.raw_treatment_effect)
        .bind(result.projected_steady_state_effect)
        .bind(result.novelty_amplitude)
        .bind(result.decay_constant_days)
        .bind(result.is_stabilized)
        .execute(&self.pool)
        .await
        .context("failed to insert into novelty_analysis_results")?;

        Ok(())
    }

    // -- Interference results -----------------------------------------------

    /// Get the latest cached interference analysis result for an experiment.
    #[allow(dead_code)]
    pub async fn get_interference_result(
        &self,
        experiment_id: &Uuid,
    ) -> Result<Option<InterferenceAnalysisResult>> {
        let row: Option<InterferenceRow> = sqlx::query_as(
            "SELECT interference_detected, js_divergence, jaccard_similarity, \
             treatment_gini, control_gini \
             FROM interference_analysis_results \
             WHERE experiment_id = $1 \
             ORDER BY computed_at DESC LIMIT 1",
        )
        .bind(experiment_id)
        .fetch_optional(&self.pool)
        .await
        .context("failed to query interference_analysis_results")?;

        match row {
            Some((
                interference_detected,
                js_divergence,
                jaccard_similarity,
                treatment_gini,
                control_gini,
            )) => Ok(Some(InterferenceAnalysisResult {
                experiment_id: experiment_id.to_string(),
                interference_detected,
                jensen_shannon_divergence: js_divergence.unwrap_or(0.0),
                jaccard_similarity_top_100: jaccard_similarity.unwrap_or(0.0),
                treatment_gini_coefficient: treatment_gini.unwrap_or(0.0),
                control_gini_coefficient: control_gini.unwrap_or(0.0),
                treatment_catalog_coverage: 0.0,
                control_catalog_coverage: 0.0,
                spillover_titles: vec![],
                computed_at: Some(super::grpc::now_timestamp()),
                feedback_loop_detected: false,
                feedback_loop_bias_estimate: 0.0,
                contamination_effect_correlation: 0.0,
                feedback_loop_computed_at: None,
            })),
            None => Ok(None),
        }
    }

    /// Save an interference analysis result to the cache.
    pub async fn save_interference_result(
        &self,
        experiment_id: &Uuid,
        result: &InterferenceAnalysisResult,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO interference_analysis_results \
             (experiment_id, interference_detected, js_divergence, jaccard_similarity, \
              treatment_gini, control_gini) \
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(experiment_id)
        .bind(result.interference_detected)
        .bind(result.jensen_shannon_divergence)
        .bind(result.jaccard_similarity_top_100)
        .bind(result.treatment_gini_coefficient)
        .bind(result.control_gini_coefficient)
        .execute(&self.pool)
        .await
        .context("failed to insert into interference_analysis_results")?;

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests (require running PostgreSQL — gated with #[ignore])
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use experimentation_proto::experimentation::analysis::v1::TitleSpillover;

    async fn test_store() -> Option<AnalysisStore> {
        let url = std::env::var("DATABASE_URL").ok()?;
        AnalysisStore::connect(&url).await.ok()
    }

    fn sample_analysis_result(experiment_id: &str) -> AnalysisResult {
        AnalysisResult {
            experiment_id: experiment_id.to_string(),
            metric_results: vec![MetricResult {
                metric_id: "ctr".into(),
                variant_id: "treatment".into(),
                control_mean: 3.0,
                treatment_mean: 13.0,
                absolute_effect: 10.0,
                relative_effect: 3.333,
                ci_lower: 7.0,
                ci_upper: 13.0,
                p_value: 0.001,
                is_significant: true,
                cuped_adjusted_effect: 9.5,
                cuped_ci_lower: 6.5,
                cuped_ci_upper: 12.5,
                variance_reduction_pct: 15.0,
                sequential_result: None,
                segment_results: vec![],
                session_level_result: None,
                ipw_result: None,
                e_value: 0.0,
                log_e_value: 0.0,
            }],
            srm_result: Some(ProtoSrmResult {
                chi_squared: 0.1,
                p_value: 0.75,
                is_mismatch: false,
                observed_counts: [("control".into(), 50), ("treatment".into(), 50)]
                    .into_iter()
                    .collect(),
                expected_counts: [("control".into(), 50), ("treatment".into(), 50)]
                    .into_iter()
                    .collect(),
            }),
            surrogate_projections: vec![],
            cochran_q_p_value: 0.0,
            computed_at: Some(super::super::grpc::now_timestamp()),
        }
    }

    #[tokio::test]
    #[ignore]
    async fn test_roundtrip_analysis_result() {
        let store = match test_store().await {
            Some(s) => s,
            None => return,
        };
        let id = Uuid::new_v4();
        let result = sample_analysis_result(&id.to_string());

        store.save_analysis_result(&id, &result).await.unwrap();
        let cached = store.get_analysis_result(&id).await.unwrap().unwrap();

        assert_eq!(cached.experiment_id, id.to_string());
        assert_eq!(cached.metric_results.len(), 1);
        let mr = &cached.metric_results[0];
        assert_eq!(mr.metric_id, "ctr");
        assert!((mr.absolute_effect - 10.0).abs() < 1e-10);
        assert!(cached.srm_result.is_some());
        assert!(!cached.srm_result.unwrap().is_mismatch);
    }

    #[tokio::test]
    #[ignore]
    async fn test_get_nonexistent_returns_none() {
        let store = match test_store().await {
            Some(s) => s,
            None => return,
        };
        let id = Uuid::new_v4();
        let result = store.get_analysis_result(&id).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    #[ignore]
    async fn test_latest_wins() {
        let store = match test_store().await {
            Some(s) => s,
            None => return,
        };
        let id = Uuid::new_v4();

        let mut result1 = sample_analysis_result(&id.to_string());
        result1.cochran_q_p_value = 0.1;
        store.save_analysis_result(&id, &result1).await.unwrap();

        let mut result2 = sample_analysis_result(&id.to_string());
        result2.cochran_q_p_value = 0.2;
        store.save_analysis_result(&id, &result2).await.unwrap();

        let cached = store.get_analysis_result(&id).await.unwrap().unwrap();
        assert!((cached.cochran_q_p_value - 0.2).abs() < 1e-10);
    }

    #[tokio::test]
    #[ignore]
    async fn test_roundtrip_novelty_result() {
        let store = match test_store().await {
            Some(s) => s,
            None => return,
        };
        let id = Uuid::new_v4();
        let result = NoveltyAnalysisResult {
            experiment_id: id.to_string(),
            metric_id: "ctr".into(),
            novelty_detected: true,
            raw_treatment_effect: 5.0,
            projected_steady_state_effect: 3.0,
            novelty_amplitude: 2.5,
            decay_constant_days: 4.0,
            is_stabilized: false,
            days_until_projected_stability: 10.0,
            computed_at: None,
        };

        store.save_novelty_result(&id, &result).await.unwrap();
        let cached = store.get_novelty_result(&id).await.unwrap().unwrap();

        assert_eq!(cached.metric_id, "ctr");
        assert!(cached.novelty_detected);
        assert!((cached.raw_treatment_effect - 5.0).abs() < 1e-10);
        assert!((cached.decay_constant_days - 4.0).abs() < 1e-10);
    }

    #[tokio::test]
    #[ignore]
    async fn test_roundtrip_interference_result() {
        let store = match test_store().await {
            Some(s) => s,
            None => return,
        };
        let id = Uuid::new_v4();
        let result = InterferenceAnalysisResult {
            experiment_id: id.to_string(),
            interference_detected: true,
            jensen_shannon_divergence: 0.12,
            jaccard_similarity_top_100: 0.65,
            treatment_gini_coefficient: 0.45,
            control_gini_coefficient: 0.30,
            treatment_catalog_coverage: 0.70,
            control_catalog_coverage: 0.85,
            spillover_titles: vec![TitleSpillover {
                content_id: "movie-42".into(),
                treatment_watch_rate: 0.15,
                control_watch_rate: 0.05,
                p_value: 0.001,
            }],
            computed_at: None,
        };

        store.save_interference_result(&id, &result).await.unwrap();
        let cached = store.get_interference_result(&id).await.unwrap().unwrap();

        assert!(cached.interference_detected);
        assert!((cached.jensen_shannon_divergence - 0.12).abs() < 1e-10);
        assert!((cached.jaccard_similarity_top_100 - 0.65).abs() < 1e-10);
    }
}
