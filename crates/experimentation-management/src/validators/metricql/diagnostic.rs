//! Diagnostic types for MetricQL semantic analysis (ADR-026 Phase 2 / #436).
//!
//! `Diagnostic` is the collect-all error type returned by `analyze`.
//! Unlike Go's `AnalyzeError` (single-error return), the Rust analyzer
//! accumulates a `Vec<Diagnostic>` so operators see all errors at once.

use super::ast::Span;

/// Severity level of a diagnostic.
///
/// Only `Error` is emitted in v1. `Warning` is reserved for future use
/// (e.g. deprecated constructs, performance hints).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    /// Reserved for v1; not emitted by the current analyzer.
    Warning,
}

/// A span-tagged semantic diagnostic. Mirrors Go `AnalyzeError{Span, Message}`
/// but is used in a collect-all `Vec<Diagnostic>` rather than a single-error
/// return, enabling the M6 editor to highlight every issue at once.
#[derive(Debug, Clone, PartialEq)]
pub struct Diagnostic {
    pub severity: Severity,
    pub message: String,
    pub span: Span,
}

impl Diagnostic {
    /// Construct an `Error`-severity diagnostic at `span`.
    pub fn error(span: Span, message: impl Into<String>) -> Self {
        Self { severity: Severity::Error, message: message.into(), span }
    }
}
