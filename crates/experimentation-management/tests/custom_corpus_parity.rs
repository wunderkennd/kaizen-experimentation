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

use std::fs;
use std::path::PathBuf;

use experimentation_management::migration::classify_and_translate;
use experimentation_proto::experimentation::common::v1::MetricDefinition;

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
/// original definition so it can copy over metadata (metric_id, name, etc.).
/// For corpus fixtures the metadata fields are intentionally blank — the
/// translator is responsible for populating them from the SQL parse result.
fn custom_metric_shell(custom_sql: &str) -> MetricDefinition {
    use experimentation_proto::experimentation::common::v1::MetricType;
    MetricDefinition {
        r#type: MetricType::Custom as i32,
        custom_sql: custom_sql.to_string(),
        ..Default::default()
    }
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
// Parity test — #[ignore] until A3/A4/A5 land
//
// When A3 ships (empty-SQL + parse-error Tier 3 classification) the test can
// be un-ignored. When A4/A5 also ship (Tier 1/2 translators), the ignore can
// be removed entirely and the test becomes a green gate.
//
// To run manually before that point:
//   cargo test -p experimentation-management --test custom_corpus_parity -- --ignored
// ---------------------------------------------------------------------------

#[test]
#[ignore = "awaiting A3 (classifier), A4 (Tier 1 translators), A5 (Tier 2 translator)"]
fn corpus_parity() {
    let fixtures = load_corpus();
    let mut failures: Vec<String> = vec![];

    for f in &fixtures {
        let original = custom_metric_shell(&f.custom_sql);
        let result = classify_and_translate(&f.custom_sql, &original);
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
// Tier-3 early smoke — should pass even with the stub classifier
//
// The stub `classify_and_translate` always returns Tier3Untranslatable, so
// Tier 3 fixtures already pass. This sub-test runs eagerly (no #[ignore]) to
// give fast feedback that the stub + corpus are correctly wired up, without
// requiring A3/A4/A5 to be implemented first.
//
// NOTE: This test will need updating when A3 lands and the stub is replaced
// with real classification (at that point all fixtures should use
// `corpus_parity` instead).
// ---------------------------------------------------------------------------

#[test]
fn tier3_fixtures_match_stub_classifier() {
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
        let original = custom_metric_shell(&f.custom_sql);
        let result = classify_and_translate(&f.custom_sql, &original);
        let got_tag = tier_tag(&result);
        assert_eq!(
            got_tag,
            "tier3_untranslatable",
            "fixture '{}': stub classifier must return tier3_untranslatable for all inputs; got '{}'",
            f.name,
            got_tag,
        );
    }
}
