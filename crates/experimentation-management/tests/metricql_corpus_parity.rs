//! Cross-implementation parity test for the MetricQL golden corpus
//! (ADR-026 Phase 2 / #436).
//!
//! Loads `test-vectors/metricql_corpus.json` and runs every fixture through
//! the Rust validator. Paired with `TestCorpusParity` in
//! `services/metrics/internal/metricql/corpus_parity_test.go` to assert both
//! implementations accept/reject identical inputs and extract the same
//! `@metric_ref` sets.

use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

use experimentation_management::validators::metricql::{validate_metricql, ValidateContext};

#[derive(serde::Deserialize)]
struct Fixture {
    name: String,
    source: String,
    valid: bool,
    #[serde(default)]
    expected_refs: Option<Vec<String>>,
    #[serde(default)]
    expected_error_count: Option<usize>,
}

#[test]
fn corpus_parity() {
    let path = corpus_path();
    let raw = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read corpus at {}: {e}", path.display()));
    let fixtures: Vec<Fixture> =
        serde_json::from_str(&raw).expect("parse corpus JSON");
    assert!(
        fixtures.len() >= 30,
        "corpus shrank unexpectedly: {} fixtures",
        fixtures.len()
    );

    let mut failures: Vec<String> = vec![];

    for f in &fixtures {
        // For valid fixtures that reference @metrics, populate the known set so
        // existence checks don't fire. For error fixtures we pass an empty set —
        // they fail at lex/parse/semantic level regardless.
        let known: HashSet<String> = f
            .expected_refs
            .clone()
            .unwrap_or_default()
            .into_iter()
            .collect();
        let ctx = ValidateContext {
            known_metric_ids: Some(&known),
        };

        let result = validate_metricql(&f.source, &ctx);

        match (f.valid, result) {
            (true, Ok(mut refs)) => {
                refs.sort();
                let mut want = f.expected_refs.clone().unwrap_or_default();
                want.sort();
                if refs != want {
                    failures.push(format!(
                        "{}: refs mismatch — want {want:?}, got {refs:?}",
                        f.name
                    ));
                }
            }
            (true, Err(diags)) => {
                failures.push(format!(
                    "{}: expected valid, got {} diagnostic(s): {:?}",
                    f.name,
                    diags.len(),
                    diags.iter().map(|d| &d.message).collect::<Vec<_>>()
                ));
            }
            (false, Ok(refs)) => {
                failures.push(format!(
                    "{}: expected invalid, got Ok({refs:?})",
                    f.name
                ));
            }
            (false, Err(diags)) => {
                if let Some(want_count) = f.expected_error_count {
                    if diags.len() != want_count {
                        failures.push(format!(
                            "{}: expected {want_count} diagnostic(s), got {} — {:?}",
                            f.name,
                            diags.len(),
                            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
                        ));
                    }
                }
            }
        }
    }

    assert!(
        failures.is_empty(),
        "corpus parity failures ({} of {} fixtures):\n  {}",
        failures.len(),
        fixtures.len(),
        failures.join("\n  ")
    );
}

/// Resolves the path to `test-vectors/metricql_corpus.json` relative to the
/// Cargo manifest dir (`crates/experimentation-management`).
///
/// Directory layout:
///   crates/experimentation-management/ (CARGO_MANIFEST_DIR)
///   └── ../../test-vectors/metricql_corpus.json
fn corpus_path() -> PathBuf {
    let manifest_dir =
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR must be set by cargo");
    PathBuf::from(manifest_dir)
        .parent()
        .expect("expected parent: crates/")
        .parent()
        .expect("expected grandparent: repo root")
        .join("test-vectors/metricql_corpus.json")
}
