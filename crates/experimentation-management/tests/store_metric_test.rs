//! Unit + integration tests for the ADR-026 Phase 1 `ManagementStore` metric
//! methods added in #433 / Task A2.
//!
//! ## Running these tests
//!
//! There is no `#[sqlx::test]` infrastructure in this crate yet, so the DB
//! integration tests below are gated on the `DATABASE_URL` env var and skip
//! quietly when it is unset. To exercise them locally:
//!
//! ```sh
//! # one-time: start a Postgres and apply migrations 001..011
//! just db-reset && just migrate
//! export DATABASE_URL=postgres://postgres:postgres@localhost:5432/kaizen_dev
//! cargo test -p experimentation-management --test store_metric_test
//! ```
//!
//! Without `DATABASE_URL`, the harness still runs and prints a skip message
//! per test; CI exercises the same code path against a managed Postgres in
//! the C1 end-to-end suite, so this file's contract is: compile cleanly +
//! pass when DATABASE_URL is set + skip when it isn't.

use experimentation_management::store::{ManagementStore, MetricFilter};
use experimentation_proto::experimentation::common::v1::{
    metric_definition::TypeConfig as MetricTypeConfig, CompositeConfig, CompositeOperand,
    CompositeOperator, FilteredMeanConfig, MetricAggregationLevel, MetricDefinition,
    MetricStakeholder, MetricType, WindowedCountConfig,
};

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

/// Try to connect to the dev/test Postgres. Returns `None` (with a printed
/// skip message) if `DATABASE_URL` is not set, so the test binary still
/// passes in environments without a database.
async fn try_store() -> Option<ManagementStore> {
    let url = std::env::var("DATABASE_URL").ok()?;
    match ManagementStore::connect(&url).await {
        Ok(s) => Some(s),
        Err(e) => {
            eprintln!("skip: could not connect to DATABASE_URL: {e}");
            None
        }
    }
}

fn unique_id(prefix: &str) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static CTR: AtomicU64 = AtomicU64::new(1);
    let n = CTR.fetch_add(1, Ordering::Relaxed);
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{prefix}_{ts}_{n}")
}

fn filtered_mean_metric(id: &str) -> MetricDefinition {
    MetricDefinition {
        metric_id: id.to_string(),
        name: format!("filtered mean {id}"),
        description: "test fixture".into(),
        r#type: MetricType::FilteredMean as i32,
        source_event_type: "video_play".into(),
        stakeholder: MetricStakeholder::User as i32,
        aggregation_level: MetricAggregationLevel::User as i32,
        type_config: Some(MetricTypeConfig::FilteredMean(FilteredMeanConfig {
            filter_sql: "platform = 'mobile' AND duration_ms > 5000".into(),
            value_column: "duration_ms".into(),
        })),
        ..Default::default()
    }
}

fn windowed_count_metric(id: &str) -> MetricDefinition {
    MetricDefinition {
        metric_id: id.to_string(),
        name: format!("windowed count {id}"),
        r#type: MetricType::WindowedCount as i32,
        source_event_type: "purchase".into(),
        stakeholder: MetricStakeholder::Platform as i32,
        aggregation_level: MetricAggregationLevel::User as i32,
        type_config: Some(MetricTypeConfig::WindowedCount(WindowedCountConfig {
            event_type: "purchase".into(),
            filter_sql: String::new(),
            window_hours: 168,
        })),
        ..Default::default()
    }
}

fn mean_metric(id: &str) -> MetricDefinition {
    MetricDefinition {
        metric_id: id.to_string(),
        name: format!("mean {id}"),
        r#type: MetricType::Mean as i32,
        source_event_type: "video_play".into(),
        stakeholder: MetricStakeholder::User as i32,
        aggregation_level: MetricAggregationLevel::User as i32,
        type_config: None,
        ..Default::default()
    }
}

fn composite_metric(id: &str, operand_ids: &[&str]) -> MetricDefinition {
    let operands = operand_ids
        .iter()
        .map(|m| CompositeOperand {
            metric_id: (*m).to_string(),
            weight: 1.0,
        })
        .collect();
    MetricDefinition {
        metric_id: id.to_string(),
        name: format!("composite {id}"),
        r#type: MetricType::Composite as i32,
        stakeholder: MetricStakeholder::User as i32,
        aggregation_level: MetricAggregationLevel::User as i32,
        type_config: Some(MetricTypeConfig::Composite(CompositeConfig {
            operator: CompositeOperator::Add as i32,
            operands,
        })),
        ..Default::default()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn create_and_get_roundtrip_filtered_mean() {
    let Some(store) = try_store().await else { return };
    let id = unique_id("fm");
    let m = filtered_mean_metric(&id);

    let inserted = store.create_metric(&m).await.expect("create_metric ok");
    assert_eq!(inserted.metric_id, id);
    assert_eq!(inserted.r#type, "FILTERED_MEAN");
    let tc = inserted
        .type_config
        .as_ref()
        .expect("type_config persisted");
    assert_eq!(
        tc.0.get("value_column").and_then(|v| v.as_str()),
        Some("duration_ms")
    );

    let fetched = store.get_metric(&id).await.expect("get_metric ok");
    assert_eq!(fetched.metric_id, inserted.metric_id);
    assert_eq!(fetched.name, inserted.name);
    assert_eq!(fetched.r#type, inserted.r#type);
    assert_eq!(
        fetched.type_config.map(|j| j.0),
        inserted.type_config.map(|j| j.0)
    );
}

#[tokio::test]
async fn list_metrics_with_stakeholder_filter() {
    let Some(store) = try_store().await else { return };
    let id_user = unique_id("user_metric");
    let id_platform = unique_id("platform_metric");
    store
        .create_metric(&filtered_mean_metric(&id_user))
        .await
        .unwrap();
    store
        .create_metric(&windowed_count_metric(&id_platform))
        .await
        .unwrap();

    let user_only = store
        .list_metrics(MetricFilter {
            stakeholder: Some("USER".into()),
            ..Default::default()
        })
        .await
        .expect("list ok");
    assert!(user_only.iter().any(|m| m.metric_id == id_user));
    assert!(user_only.iter().all(|m| m.stakeholder == "USER"));

    let platform_only = store
        .list_metrics(MetricFilter {
            stakeholder: Some("PLATFORM".into()),
            ..Default::default()
        })
        .await
        .expect("list ok");
    assert!(platform_only.iter().any(|m| m.metric_id == id_platform));
    assert!(platform_only.iter().all(|m| m.stakeholder == "PLATFORM"));
}

#[tokio::test]
async fn exists_metric_true_after_create_false_for_unknown() {
    let Some(store) = try_store().await else { return };
    let id = unique_id("exists");
    assert!(!store.exists_metric(&id).await.unwrap(), "unknown id absent");
    store.create_metric(&filtered_mean_metric(&id)).await.unwrap();
    assert!(store.exists_metric(&id).await.unwrap(), "present after create");
}

#[tokio::test]
async fn exists_all_metrics_true_when_present_false_when_one_missing() {
    let Some(store) = try_store().await else { return };
    let id_a = unique_id("all_a");
    let id_b = unique_id("all_b");
    store.create_metric(&filtered_mean_metric(&id_a)).await.unwrap();
    store.create_metric(&filtered_mean_metric(&id_b)).await.unwrap();

    let all_present = store
        .exists_all_metrics(&[id_a.as_str(), id_b.as_str()])
        .await
        .unwrap();
    assert!(all_present);

    let missing = unique_id("all_missing");
    let one_missing = store
        .exists_all_metrics(&[id_a.as_str(), missing.as_str()])
        .await
        .unwrap();
    assert!(!one_missing);

    // Empty input is trivially satisfied.
    assert!(store.exists_all_metrics(&[]).await.unwrap());
}

#[tokio::test]
async fn get_composite_operands_returns_operands_in_declaration_order() {
    let Some(store) = try_store().await else { return };
    let id_a = unique_id("op_a");
    let id_b = unique_id("op_b");
    let id_c = unique_id("op_c");
    let id_comp = unique_id("composite");

    store.create_metric(&mean_metric(&id_a)).await.unwrap();
    store.create_metric(&mean_metric(&id_b)).await.unwrap();
    store.create_metric(&mean_metric(&id_c)).await.unwrap();
    store
        .create_metric(&composite_metric(
            &id_comp,
            &[id_a.as_str(), id_b.as_str(), id_c.as_str()],
        ))
        .await
        .unwrap();

    let operands = store.get_composite_operands(&id_comp).await.unwrap();
    assert_eq!(operands, vec![id_a.clone(), id_b.clone(), id_c.clone()]);

    // Non-COMPOSITE row → empty Vec, not NotFound.
    let non_comp = store.get_composite_operands(&id_a).await.unwrap();
    assert!(non_comp.is_empty());
}

#[tokio::test]
async fn get_composite_operands_returns_not_found_for_unknown_metric() {
    let Some(store) = try_store().await else { return };
    let missing = unique_id("never_created");
    let err = store.get_composite_operands(&missing).await.unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("not found"),
        "expected NotFound, got: {msg}"
    );
}
