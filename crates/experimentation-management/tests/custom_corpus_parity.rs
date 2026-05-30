//! Corpus parity test for the CUSTOM-metric migration classifier (ADR-026 Phase 3 / #437).
//!
//! Loads `test-vectors/custom_migration_corpus.json` and drives every fixture
//! through `classify_and_translate`, asserting that the returned tier tag
//! matches the corpus expectation.
//!
//! ## Current state
//!
//! All `#[ignore]`-marked tests below will be **re-enabled progressively**:
//!
//! | Task | Enables                                   |
//! |------|-------------------------------------------|
//! | A3   | Tier 3 parse-error and empty-SQL cases    |
//! | A4   | Tier 1 (FILTERED_MEAN/COMPOSITE/WINDOWED) |
//! | A5   | Tier 2 (METRICQL)                         |
//!
//! The `corpus_loads_and_counts_fixtures` smoke test is **not** ignored — it
//! runs on every CI pass and guards against accidental corpus truncation.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use experimentation_management::migration::classify_and_translate;
use experimentation_management::store::StoreError;
use experimentation_management::validators::MetricLookup;
use experimentation_proto::experimentation::common::v1::{MetricDefinition, MetricType};

// ---------------------------------------------------------------------------
// Fixture schema
// ---------------------------------------------------------------------------

#[derive(serde::Deserialize)]
struct Fixture {
    name: String,
    custom_sql: String,
    expected_tier: String,
    expected_proposal: Option<serde_json::Value>,
    expected_reason: String,
}

// ---------------------------------------------------------------------------
// Local MetricLookup implementations for testing
// ---------------------------------------------------------------------------

/// Seeded lookup — knows a fixed set of metric IDs as leaves (no operands).
/// Used for COMPOSITE fixture tests where the validator checks operand existence.
struct SeedLookup {
    ids: HashMap<String, ()>,
}

impl SeedLookup {
    fn with_ids(ids: &[&str]) -> Self {
        Self {
            ids: ids.iter().map(|s| ((*s).to_string(), ())).collect(),
        }
    }
}

#[tonic::async_trait]
impl MetricLookup for SeedLookup {
    async fn exists_all_metrics(&self, metric_ids: &[&str]) -> Result<bool, StoreError> {
        Ok(metric_ids.iter().all(|id| self.ids.contains_key(*id)))
    }

    async fn get_composite_operands(
        &self,
        _metric_id: &str,
    ) -> Result<Vec<String>, StoreError> {
        // All seeded metrics are leaves (no sub-operands).
        Ok(vec![])
    }

    async fn get_metricql_refs(&self, _metric_id: &str) -> Result<Vec<String>, StoreError> {
        Ok(vec![])
    }

    async fn get_metric_type(
        &self,
        metric_id: &str,
    ) -> Result<MetricType, StoreError> {
        if self.ids.contains_key(metric_id) {
            Ok(MetricType::FilteredMean) // leaf type; cycle detection won't follow
        } else {
            Err(StoreError::NotFound(metric_id.to_string()))
        }
    }
}

// ---------------------------------------------------------------------------
// Tier-tag helper
//
// Maps a `ClassificationResult` to the compact string tag used in the corpus.
// This is the canonical mapping — A3/A4/A5 must not break it.
// ---------------------------------------------------------------------------

fn tier_tag(result: &experimentation_management::migration::ClassificationResult) -> &'static str {
    use experimentation_management::migration::ClassificationResult;
    match result {
        ClassificationResult::Tier1Filtered { .. } => "tier1_filtered_mean",
        ClassificationResult::Tier1Composite { .. } => "tier1_composite",
        ClassificationResult::Tier1WindowedCount { .. } => "tier1_windowed_count",
        ClassificationResult::Tier2Metricql { .. } => "tier2_metricql",
        ClassificationResult::Tier3Untranslatable { .. } => "tier3_untranslatable",
    }
}

// ---------------------------------------------------------------------------
// Corpus path
//
// Resolves `test-vectors/custom_migration_corpus.json` from CARGO_MANIFEST_DIR.
// Directory layout:
//   crates/experimentation-management/   (CARGO_MANIFEST_DIR)
//   └── ../../test-vectors/custom_migration_corpus.json
// ---------------------------------------------------------------------------

fn corpus_path() -> PathBuf {
    let manifest_dir =
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR must be set by cargo");
    PathBuf::from(manifest_dir)
        .parent()
        .expect("expected parent: crates/")
        .parent()
        .expect("expected grandparent: repo root")
        .join("test-vectors/custom_migration_corpus.json")
}

fn load_corpus() -> Vec<Fixture> {
    let path = corpus_path();
    let raw = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read corpus at {}: {e}", path.display()));
    serde_json::from_str(&raw).expect("parse custom_migration_corpus.json")
}

/// Build a minimal `MetricDefinition` for driving `classify_and_translate`.
///
/// The original metric is CUSTOM-typed (type = 6). The translator receives the
/// original definition so it can copy metadata (metric_id, name, etc.) into
/// the proposal. We use `fixture_name` as both metric_id and name so that the
/// round-trip `validate_metric_definition` call inside `translate` can pass the
/// common-fields validator (which requires non-empty metric_id and name).
///
/// In production, CUSTOM metrics always have non-empty ids/names. Using the
/// fixture name here mirrors real usage without distorting the translation test.
fn custom_metric_shell(custom_sql: &str, fixture_name: &str) -> MetricDefinition {
    MetricDefinition {
        metric_id: fixture_name.to_string(),
        name: fixture_name.to_string(),
        r#type: MetricType::Custom as i32,
        custom_sql: custom_sql.to_string(),
        ..Default::default()
    }
}

/// Build a `SeedLookup` pre-seeded with operand metric IDs extracted from the
/// corpus fixture's `expected_proposal.composite.operands[].metric_id`.
///
/// For FILTERED_MEAN and WINDOWED_COUNT fixtures, the lookup is empty (those
/// validators don't call `exists_all_metrics` for non-empty slices).
fn lookup_for_fixture(fixture: &Fixture) -> SeedLookup {
    let mut ids: Vec<String> = Vec::new();
    if let Some(ref proposal) = fixture.expected_proposal {
        if let Some(operands) = proposal.get("composite").and_then(|c| c.get("operands")) {
            if let Some(arr) = operands.as_array() {
                for op in arr {
                    if let Some(mid) = op.get("metric_id").and_then(|v| v.as_str()) {
                        ids.push(mid.to_string());
                    }
                }
            }
        }
    }
    let id_refs: Vec<&str> = ids.iter().map(|s| s.as_str()).collect();
    SeedLookup::with_ids(&id_refs)
}

// ---------------------------------------------------------------------------
// Smoke test — always runs, guards corpus integrity
// ---------------------------------------------------------------------------

#[test]
fn corpus_loads_and_counts_fixtures() {
    let fixtures = load_corpus();

    // Exact count mandated by the task spec.
    assert_eq!(
        fixtures.len(),
        30,
        "corpus must have exactly 30 fixtures; found {}",
        fixtures.len()
    );

    // Distribution check: count by expected_tier.
    let mut fm = 0usize;
    let mut composite = 0usize;
    let mut windowed = 0usize;
    let mut metricql = 0usize;
    let mut tier3 = 0usize;

    for f in &fixtures {
        match f.expected_tier.as_str() {
            "tier1_filtered_mean" => fm += 1,
            "tier1_composite" => composite += 1,
            "tier1_windowed_count" => windowed += 1,
            "tier2_metricql" => metricql += 1,
            "tier3_untranslatable" => tier3 += 1,
            other => panic!(
                "fixture '{}': unknown expected_tier '{}'",
                f.name, other
            ),
        }
    }

    assert_eq!(fm, 5, "expected 5 FILTERED_MEAN fixtures, found {fm}");
    assert_eq!(composite, 3, "expected 3 COMPOSITE fixtures, found {composite}");
    assert_eq!(windowed, 2, "expected 2 WINDOWED_COUNT fixtures, found {windowed}");
    assert_eq!(metricql, 10, "expected 10 METRICQL fixtures, found {metricql}");
    assert_eq!(tier3, 10, "expected 10 Tier 3 fixtures, found {tier3}");

    // Every fixture must have a non-empty name, non-empty expected_reason.
    for f in &fixtures {
        assert!(!f.name.is_empty(), "fixture name must not be empty");
        assert!(
            !f.expected_reason.is_empty(),
            "fixture '{}': expected_reason must not be empty",
            f.name
        );
        // Tier 3 fixtures must have null proposal; Tier 1/2 must have Some.
        if f.expected_tier == "tier3_untranslatable" {
            assert!(
                f.expected_proposal.is_none(),
                "fixture '{}': Tier 3 must have null expected_proposal",
                f.name
            );
        } else {
            assert!(
                f.expected_proposal.is_some(),
                "fixture '{}': Tier 1/2 must have non-null expected_proposal",
                f.name
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Parity test — #[ignore] until A5 lands (Tier 2 METRICQL translator)
//
// A4 is now landed. Once A5 ships, remove the #[ignore] and this test
// becomes the full green gate.
//
// To run manually:
//   cargo test -p experimentation-management --test custom_corpus_parity -- --ignored
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "awaiting A5 (Tier 2 METRICQL translator)"]
async fn corpus_parity() {
    let fixtures = load_corpus();
    let mut failures: Vec<String> = vec![];

    for f in &fixtures {
        let original = custom_metric_shell(&f.custom_sql, &f.name);
        let lookup = lookup_for_fixture(f);
        let result = classify_and_translate(&f.custom_sql, &original, &lookup).await;
        let got_tag = tier_tag(&result);

        if got_tag != f.expected_tier.as_str() {
            failures.push(format!(
                "'{}': expected tier '{}', got '{}'",
                f.name, f.expected_tier, got_tag,
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "corpus parity failures ({} of {} fixtures):\n  {}",
        failures.len(),
        fixtures.len(),
        failures.join("\n  "),
    );
}

// ---------------------------------------------------------------------------
// Tier-3 smoke — verifies all Tier 3 corpus fixtures still classify Tier 3
// after A4.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn tier3_fixtures_match_stub_classifier() {
    let fixtures = load_corpus();
    let tier3_fixtures: Vec<&Fixture> = fixtures
        .iter()
        .filter(|f| f.expected_tier == "tier3_untranslatable")
        .collect();

    assert!(
        !tier3_fixtures.is_empty(),
        "expected at least one tier3 fixture"
    );

    for f in tier3_fixtures {
        let original = custom_metric_shell(&f.custom_sql, &f.name);
        let lookup = SeedLookup::with_ids(&[]);
        let result = classify_and_translate(&f.custom_sql, &original, &lookup).await;
        let got_tag = tier_tag(&result);
        assert_eq!(
            got_tag,
            "tier3_untranslatable",
            "fixture '{}': must return tier3_untranslatable; got '{}'",
            f.name,
            got_tag,
        );
    }
}

// ---------------------------------------------------------------------------
// Tier 1 fixtures — active (no ignore), must all translate successfully.
//
// This test verifies all 10 Tier 1 corpus fixtures (5 FILTERED_MEAN +
// 3 COMPOSITE + 2 WINDOWED_COUNT) produce the correct tier tag.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn tier1_fixtures_translate_successfully() {
    let fixtures = load_corpus();
    let tier1_fixtures: Vec<&Fixture> = fixtures
        .iter()
        .filter(|f| f.expected_tier.starts_with("tier1_"))
        .collect();

    assert_eq!(
        tier1_fixtures.len(),
        10,
        "expected 10 Tier 1 fixtures, found {}",
        tier1_fixtures.len()
    );

    let mut failures: Vec<String> = vec![];

    for f in &tier1_fixtures {
        let original = custom_metric_shell(&f.custom_sql, &f.name);
        let lookup = lookup_for_fixture(f);
        let result = classify_and_translate(&f.custom_sql, &original, &lookup).await;
        let got_tag = tier_tag(&result);

        if got_tag != f.expected_tier.as_str() {
            failures.push(format!(
                "'{}': expected '{}', got '{}'",
                f.name, f.expected_tier, got_tag,
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "Tier 1 translation failures ({} of {} fixtures):\n  {}",
        failures.len(),
        tier1_fixtures.len(),
        failures.join("\n  "),
    );
}
