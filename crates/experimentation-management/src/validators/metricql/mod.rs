//! MetricQL parser/analyzer for M5 server-side validation (ADR-026 Phase 2 / #436).
//! Mirrors `services/metrics/internal/metricql/` Go implementation; corpus parity
//! enforced via `test-vectors/metricql_corpus.json`.

pub mod analyze;
pub mod ast;
pub mod diagnostic;
pub mod lexer;
pub mod parser;
