//! MetricQL lexer — direct port of
//! `services/metrics/internal/metricql/lexer.go` (ADR-026 Phase 2 / #436).
//!
//! Design invariants from Lock 1 (`#559`):
//!
//!   - `TokMinus` is **always** its own token; `tokenize` never consumes `-`
//!     as the sign of a number. Unary minus is handled by `parseUnary` (A3).
//!   - Identifiers are ASCII lowercase only: `[a-z_][a-z0-9_]*`. Any uppercase
//!     character in identifier position is a `LexError`.
//!   - Strings are single-quoted with no escape sequences (Lock 1 v1).
//!   - Numbers are unsigned: `[0-9]+(.[0-9]+)?`.
//!   - `=` is equality; `==` does not exist in MetricQL.
//!   - `@` is its own token; the parser (A3) assembles `@ident` as a sequence.
//!   - The token stream always ends with a zero-width `Eof` at `[len, len)`.

use std::collections::HashSet;
use std::sync::OnceLock;

use super::ast::Span;

// ---------------------------------------------------------------------------
// Token kind
// ---------------------------------------------------------------------------

/// Every token type the lexer can produce.
///
/// The exhaustive-match helper test `token_kind_all_variants_named` below will
/// fail to compile if a new variant is added without a corresponding arm,
/// catching enum growth before it reaches the parser.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenKind {
    Eof,
    Ident,
    Number,
    String,
    Keyword,
    At,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Comma,
    Dot,
    Plus,
    Minus,
    Star,
    Slash,
    Eq,
    Neq,
    Lt,
    Lte,
    Gt,
    Gte,
}

// ---------------------------------------------------------------------------
// Token
// ---------------------------------------------------------------------------

/// A single lexed unit of MetricQL source. Mirrors Go `Token{Kind, Value, Span}`.
///
/// `value` is the raw source slice; for strings it **includes** the surrounding
/// single quotes (e.g., `"'mobile'"`). The parser/semantic layer strips quotes.
#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    /// Raw source text for this token (may include delimiters like quotes).
    pub value: std::string::String,
    pub span: Span,
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// A span-tagged lexer error. Source context is intentionally **not** embedded
/// here; the entry-point caller (`validate_metricql`, A5) attaches source
/// context when building a `Diagnostic`. This differs from the Go side, which
/// embeds `Source` because it formats its own messages.
#[derive(Debug, Clone, PartialEq)]
pub struct LexError {
    pub span: Span,
    pub message: std::string::String,
}

impl std::fmt::Display for LexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "metricql lex error at offset {}: {}", self.span.start, self.message)
    }
}

impl std::error::Error for LexError {}

// ---------------------------------------------------------------------------
// Reserved keywords (Lock 1)
// ---------------------------------------------------------------------------

fn reserved_keywords() -> &'static HashSet<&'static str> {
    static SET: OnceLock<HashSet<&'static str>> = OnceLock::new();
    SET.get_or_init(|| {
        [
            "where",
            "and",
            "in",
            "within",
            "of",
            "exposure",
            "hours",
            "days",
            "ratio",
            "mean",
            "sum",
            "count",
            "count_distinct",
            "proportion",
            "percentile",
        ]
        .into_iter()
        .collect()
    })
}

// ---------------------------------------------------------------------------
// Lexer
// ---------------------------------------------------------------------------

/// Tokenizes MetricQL source. Construct with [`Lexer::new`], consume with
/// [`Lexer::tokenize`]. Use the top-level [`tokenize`] convenience function
/// when you don't need to retain the struct.
pub struct Lexer {
    source: std::string::String,
    pos: usize,
    tokens: Vec<Token>,
}

impl Lexer {
    /// Create a new `Lexer` over `source`.
    pub fn new(source: impl Into<std::string::String>) -> Self {
        Self { source: source.into(), pos: 0, tokens: Vec::new() }
    }

    /// Run the lexer end-to-end.
    ///
    /// On success returns the full token stream (always ends with `Eof`).
    /// On failure returns the first `LexError` encountered; tokens produced
    /// before the error are discarded (same as Go — caller receives either a
    /// complete stream or an error, never a partial stream).
    pub fn tokenize(mut self) -> Result<Vec<Token>, LexError> {
        while self.pos < self.source.len() {
            // Index through len() + single byte at self.pos — no long-lived borrow.
            let c = self.source.as_bytes()[self.pos];

            // ── Whitespace ────────────────────────────────────────────────
            if matches!(c, b' ' | b'\t' | b'\n' | b'\r') {
                self.pos += 1;
                continue;
            }

            let start = self.pos;

            // ── Single-char and two-char tokens ───────────────────────────
            match c {
                b'@' => {
                    self.emit(TokenKind::At, start, self.pos + 1);
                    self.pos += 1;
                }
                b'(' => {
                    self.emit(TokenKind::LParen, start, self.pos + 1);
                    self.pos += 1;
                }
                b')' => {
                    self.emit(TokenKind::RParen, start, self.pos + 1);
                    self.pos += 1;
                }
                b'[' => {
                    self.emit(TokenKind::LBracket, start, self.pos + 1);
                    self.pos += 1;
                }
                b']' => {
                    self.emit(TokenKind::RBracket, start, self.pos + 1);
                    self.pos += 1;
                }
                b',' => {
                    self.emit(TokenKind::Comma, start, self.pos + 1);
                    self.pos += 1;
                }
                b'.' => {
                    self.emit(TokenKind::Dot, start, self.pos + 1);
                    self.pos += 1;
                }
                b'+' => {
                    self.emit(TokenKind::Plus, start, self.pos + 1);
                    self.pos += 1;
                }
                b'-' => {
                    // Lock 1: minus is NEVER consumed as sign of a number.
                    // Unary minus is parseUnary's job (A3).
                    self.emit(TokenKind::Minus, start, self.pos + 1);
                    self.pos += 1;
                }
                b'*' => {
                    self.emit(TokenKind::Star, start, self.pos + 1);
                    self.pos += 1;
                }
                b'/' => {
                    self.emit(TokenKind::Slash, start, self.pos + 1);
                    self.pos += 1;
                }
                b'=' => {
                    // MetricQL uses `=` for equality; `==` does not exist.
                    self.emit(TokenKind::Eq, start, self.pos + 1);
                    self.pos += 1;
                }
                b'!' => {
                    let next_is_eq = self.pos + 1 < self.source.len()
                        && self.source.as_bytes()[self.pos + 1] == b'=';
                    if next_is_eq {
                        self.emit(TokenKind::Neq, start, self.pos + 2);
                        self.pos += 2;
                    } else {
                        return Err(self.err_at(start, "unexpected '!' (expected '!=')"));
                    }
                }
                b'<' => {
                    let next_is_eq = self.pos + 1 < self.source.len()
                        && self.source.as_bytes()[self.pos + 1] == b'=';
                    if next_is_eq {
                        self.emit(TokenKind::Lte, start, self.pos + 2);
                        self.pos += 2;
                    } else {
                        self.emit(TokenKind::Lt, start, self.pos + 1);
                        self.pos += 1;
                    }
                }
                b'>' => {
                    let next_is_eq = self.pos + 1 < self.source.len()
                        && self.source.as_bytes()[self.pos + 1] == b'=';
                    if next_is_eq {
                        self.emit(TokenKind::Gte, start, self.pos + 2);
                        self.pos += 2;
                    } else {
                        self.emit(TokenKind::Gt, start, self.pos + 1);
                        self.pos += 1;
                    }
                }
                b'\'' => {
                    self.lex_string(start)?;
                }
                // ── Multi-char: NUMBER ─────────────────────────────────────
                b'0'..=b'9' => {
                    self.lex_number(start);
                }
                // ── Multi-char: IDENT / KEYWORD ───────────────────────────
                b'a'..=b'z' | b'_' => {
                    self.lex_ident_or_keyword(start);
                }
                // ── Everything else is an error ───────────────────────────
                _ => {
                    return Err(self.err_at(
                        start,
                        &format!("unexpected character {:?}", c as char),
                    ));
                }
            }
        }

        // Always append zero-width EOF at the end of the stream.
        let eof_pos = self.source.len();
        self.tokens.push(Token {
            kind: TokenKind::Eof,
            value: std::string::String::new(),
            span: Span::new(eof_pos, eof_pos),
        });
        Ok(self.tokens)
    }

    // ── Internal helpers ─────────────────────────────────────────────────────

    fn emit(&mut self, kind: TokenKind, start: usize, end: usize) {
        let value = self.source[start..end].to_owned();
        self.tokens.push(Token { kind, value, span: Span::new(start, end) });
    }

    /// Consume `[0-9]+ ('.' [0-9]+)?`.
    ///
    /// A trailing `.` without a following digit is NOT consumed — it is
    /// reserved for field access (`heartbeat.value`). This mirrors the Go
    /// lookahead behavior exactly.
    fn lex_number(&mut self, start: usize) {
        while self.pos < self.source.len()
            && self.source.as_bytes()[self.pos].is_ascii_digit()
        {
            self.pos += 1;
        }
        // Optional fractional part — only consume '.' when followed by a digit.
        if self.pos < self.source.len()
            && self.source.as_bytes()[self.pos] == b'.'
            && self.pos + 1 < self.source.len()
            && self.source.as_bytes()[self.pos + 1].is_ascii_digit()
        {
            self.pos += 1; // consume '.'
            while self.pos < self.source.len()
                && self.source.as_bytes()[self.pos].is_ascii_digit()
            {
                self.pos += 1;
            }
        }
        self.emit(TokenKind::Number, start, self.pos);
    }

    /// Consume `'` [^']* `'`.
    ///
    /// No escape sequences (Lock 1 v1). `\n` inside a string is a literal
    /// two-character sequence. Unterminated string → `LexError`.
    fn lex_string(&mut self, start: usize) -> Result<(), LexError> {
        self.pos += 1; // consume opening '
        while self.pos < self.source.len() && self.source.as_bytes()[self.pos] != b'\'' {
            self.pos += 1;
        }
        if self.pos >= self.source.len() {
            return Err(self.err_at(start, "unterminated string literal"));
        }
        self.pos += 1; // consume closing '
        self.emit(TokenKind::String, start, self.pos);
        Ok(())
    }

    /// Consume `[a-z_][a-z0-9_]*` and reclassify reserved words to `Keyword`.
    ///
    /// Uppercase characters stop identifier scanning; the outer dispatch loop
    /// will emit a `LexError` because they match the catch-all `_` arm.
    fn lex_ident_or_keyword(&mut self, start: usize) {
        while self.pos < self.source.len() {
            match self.source.as_bytes()[self.pos] {
                b'a'..=b'z' | b'0'..=b'9' | b'_' => self.pos += 1,
                _ => break,
            }
        }
        let end = self.pos;
        let text = &self.source[start..end];
        let kind = if reserved_keywords().contains(text) {
            TokenKind::Keyword
        } else {
            TokenKind::Ident
        };
        self.emit(kind, start, end);
    }

    fn err_at(&self, pos: usize, msg: &str) -> LexError {
        LexError { span: Span::new(pos, pos + 1), message: msg.to_owned() }
    }
}

// ---------------------------------------------------------------------------
// Convenience top-level function
// ---------------------------------------------------------------------------

/// Tokenize `source` and return the token stream, or the first `LexError`.
///
/// Equivalent to `Lexer::new(source).tokenize()`.
pub fn tokenize(source: &str) -> Result<Vec<Token>, LexError> {
    Lexer::new(source).tokenize()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ── Exhaustive-match helper — catches enum growth ────────────────────────

    /// Return a canonical name for every `TokenKind` variant.
    ///
    /// The match has **no wildcard**; the compiler emits E0004 if a variant is
    /// added without a corresponding arm. This proves the enum is closed and
    /// complete without testing any runtime values.
    fn token_kind_name(k: TokenKind) -> &'static str {
        match k {
            TokenKind::Eof => "Eof",
            TokenKind::Ident => "Ident",
            TokenKind::Number => "Number",
            TokenKind::String => "String",
            TokenKind::Keyword => "Keyword",
            TokenKind::At => "At",
            TokenKind::LParen => "LParen",
            TokenKind::RParen => "RParen",
            TokenKind::LBracket => "LBracket",
            TokenKind::RBracket => "RBracket",
            TokenKind::Comma => "Comma",
            TokenKind::Dot => "Dot",
            TokenKind::Plus => "Plus",
            TokenKind::Minus => "Minus",
            TokenKind::Star => "Star",
            TokenKind::Slash => "Slash",
            TokenKind::Eq => "Eq",
            TokenKind::Neq => "Neq",
            TokenKind::Lt => "Lt",
            TokenKind::Lte => "Lte",
            TokenKind::Gt => "Gt",
            TokenKind::Gte => "Gte",
        }
    }

    #[test]
    fn token_kind_all_variants_named() {
        // Verify every variant maps to a non-empty name (trivial assertion, but
        // the real value is that this test won't compile if a variant is missing).
        let all = [
            TokenKind::Eof, TokenKind::Ident, TokenKind::Number, TokenKind::String,
            TokenKind::Keyword, TokenKind::At, TokenKind::LParen, TokenKind::RParen,
            TokenKind::LBracket, TokenKind::RBracket, TokenKind::Comma, TokenKind::Dot,
            TokenKind::Plus, TokenKind::Minus, TokenKind::Star, TokenKind::Slash,
            TokenKind::Eq, TokenKind::Neq, TokenKind::Lt, TokenKind::Lte,
            TokenKind::Gt, TokenKind::Gte,
        ];
        for k in all {
            assert!(!token_kind_name(k).is_empty());
        }
    }

    // ── Table-driven happy-path tests ────────────────────────────────────────

    struct Case<'a> {
        name: &'a str,
        input: &'a str,
        /// Expected `(kind, value)` pairs for every non-EOF token.
        /// EOF is always implicit at the end.
        expected: Vec<(TokenKind, &'a str)>,
    }

    fn run_cases(cases: &[Case]) {
        for c in cases {
            let tokens = tokenize(c.input)
                .unwrap_or_else(|e| panic!("case {:?}: unexpected error: {}", c.name, e));

            // Last token must always be Eof.
            let last = tokens.last().expect("token stream is empty");
            assert_eq!(
                last.kind,
                TokenKind::Eof,
                "case {:?}: last token is not Eof",
                c.name
            );

            let non_eof: Vec<&Token> = tokens.iter().filter(|t| t.kind != TokenKind::Eof).collect();

            assert_eq!(
                non_eof.len(),
                c.expected.len(),
                "case {:?}: expected {} non-Eof tokens, got {}\n  tokens: {:?}",
                c.name,
                c.expected.len(),
                non_eof.len(),
                non_eof.iter().map(|t| (t.kind, &t.value)).collect::<Vec<_>>()
            );

            for (i, (tok, &(exp_kind, exp_value))) in non_eof.iter().zip(c.expected.iter()).enumerate() {
                assert_eq!(
                    tok.kind, exp_kind,
                    "case {:?} token[{}]: expected kind {:?}, got {:?}",
                    c.name, i, exp_kind, tok.kind
                );
                assert_eq!(
                    tok.value.as_str(), exp_value,
                    "case {:?} token[{}]: expected value {:?}, got {:?}",
                    c.name, i, exp_value, tok.value
                );
            }
        }
    }

    #[test]
    fn happy_path_table() {
        let cases = vec![
            // 1. Empty string → just Eof
            Case { name: "empty", input: "", expected: vec![] },

            // 2. Whitespace only → just Eof
            Case { name: "whitespace_only", input: "   \t\n\r", expected: vec![] },

            // 3. Reserved keyword: mean → Keyword
            Case {
                name: "keyword_mean",
                input: "mean",
                expected: vec![(TokenKind::Keyword, "mean")],
            },

            // 4. Non-keyword identifier: watch_time → Ident
            Case {
                name: "ident_watch_time",
                input: "watch_time",
                expected: vec![(TokenKind::Ident, "watch_time")],
            },

            // 5. @metric_ref → At + Ident (@ is its own token)
            Case {
                name: "at_metric_ref",
                input: "@metric_ref",
                expected: vec![
                    (TokenKind::At, "@"),
                    (TokenKind::Ident, "metric_ref"),
                ],
            },

            // 6. Integer
            Case {
                name: "integer",
                input: "42",
                expected: vec![(TokenKind::Number, "42")],
            },

            // 7. Float
            Case {
                name: "float",
                input: "0.95",
                expected: vec![(TokenKind::Number, "0.95")],
            },

            // 8. Field access: heartbeat.value → Ident + Dot + Ident
            Case {
                name: "field_access",
                input: "heartbeat.value",
                expected: vec![
                    (TokenKind::Ident, "heartbeat"),
                    (TokenKind::Dot, "."),
                    (TokenKind::Ident, "value"),
                ],
            },

            // 9. Number followed by trailing dot: 42. → Number("42") + Dot
            Case {
                name: "number_trailing_dot",
                input: "42.",
                expected: vec![
                    (TokenKind::Number, "42"),
                    (TokenKind::Dot, "."),
                ],
            },

            // 10. Single-quoted string (value INCLUDES quotes)
            Case {
                name: "string_single_quoted",
                input: "'mobile'",
                expected: vec![(TokenKind::String, "'mobile'")],
            },

            // 11. All comparison operators
            Case {
                name: "comparisons",
                input: "< <= > >= = !=",
                expected: vec![
                    (TokenKind::Lt, "<"),
                    (TokenKind::Lte, "<="),
                    (TokenKind::Gt, ">"),
                    (TokenKind::Gte, ">="),
                    (TokenKind::Eq, "="),
                    (TokenKind::Neq, "!="),
                ],
            },

            // 12. Brackets and punctuation
            Case {
                name: "brackets_punct",
                input: "() [] , .",
                expected: vec![
                    (TokenKind::LParen, "("),
                    (TokenKind::RParen, ")"),
                    (TokenKind::LBracket, "["),
                    (TokenKind::RBracket, "]"),
                    (TokenKind::Comma, ","),
                    (TokenKind::Dot, "."),
                ],
            },

            // 13. Arithmetic operators
            Case {
                name: "arithmetic",
                input: "+ - * /",
                expected: vec![
                    (TokenKind::Plus, "+"),
                    (TokenKind::Minus, "-"),
                    (TokenKind::Star, "*"),
                    (TokenKind::Slash, "/"),
                ],
            },

            // 14. CRITICAL — Lock 1 invariant: `-3` → Minus + Number (TWO tokens)
            Case {
                name: "negative_number_is_two_tokens",
                input: "-3",
                expected: vec![
                    (TokenKind::Minus, "-"),
                    (TokenKind::Number, "3"),
                ],
            },

            // 15. Full MetricQL expression (spot-check token sequence)
            Case {
                name: "full_expression",
                input: "mean(heartbeat.value) where platform = 'mobile' within 24 hours of exposure",
                expected: vec![
                    (TokenKind::Keyword, "mean"),
                    (TokenKind::LParen, "("),
                    (TokenKind::Ident, "heartbeat"),
                    (TokenKind::Dot, "."),
                    (TokenKind::Ident, "value"),
                    (TokenKind::RParen, ")"),
                    (TokenKind::Keyword, "where"),
                    (TokenKind::Ident, "platform"),
                    (TokenKind::Eq, "="),
                    (TokenKind::String, "'mobile'"),
                    (TokenKind::Keyword, "within"),
                    (TokenKind::Number, "24"),
                    (TokenKind::Keyword, "hours"),
                    (TokenKind::Keyword, "of"),
                    (TokenKind::Keyword, "exposure"),
                ],
            },

            // 16. All reserved keywords
            Case {
                name: "all_keywords",
                input: "where and in within of exposure hours days ratio mean sum count count_distinct proportion percentile",
                expected: vec![
                    (TokenKind::Keyword, "where"),
                    (TokenKind::Keyword, "and"),
                    (TokenKind::Keyword, "in"),
                    (TokenKind::Keyword, "within"),
                    (TokenKind::Keyword, "of"),
                    (TokenKind::Keyword, "exposure"),
                    (TokenKind::Keyword, "hours"),
                    (TokenKind::Keyword, "days"),
                    (TokenKind::Keyword, "ratio"),
                    (TokenKind::Keyword, "mean"),
                    (TokenKind::Keyword, "sum"),
                    (TokenKind::Keyword, "count"),
                    (TokenKind::Keyword, "count_distinct"),
                    (TokenKind::Keyword, "proportion"),
                    (TokenKind::Keyword, "percentile"),
                ],
            },

            // 17. Empty string literal
            Case {
                name: "empty_string",
                input: "''",
                expected: vec![(TokenKind::String, "''")],
            },

            // 18. Percentile call with number argument
            Case {
                name: "percentile_call",
                input: "percentile(heartbeat.value, 95)",
                expected: vec![
                    (TokenKind::Keyword, "percentile"),
                    (TokenKind::LParen, "("),
                    (TokenKind::Ident, "heartbeat"),
                    (TokenKind::Dot, "."),
                    (TokenKind::Ident, "value"),
                    (TokenKind::Comma, ","),
                    (TokenKind::Number, "95"),
                    (TokenKind::RParen, ")"),
                ],
            },

            // 19. Ratio expression with @-refs
            Case {
                name: "ratio_expression",
                input: "ratio(@revenue, @sessions)",
                expected: vec![
                    (TokenKind::Keyword, "ratio"),
                    (TokenKind::LParen, "("),
                    (TokenKind::At, "@"),
                    (TokenKind::Ident, "revenue"),
                    (TokenKind::Comma, ","),
                    (TokenKind::At, "@"),
                    (TokenKind::Ident, "sessions"),
                    (TokenKind::RParen, ")"),
                ],
            },
        ];

        run_cases(&cases);
    }

    // ── Span assertion tests (verify byte offsets) ───────────────────────────

    #[test]
    fn span_at_token_is_zero_width_at_zero() {
        let tokens = tokenize("@watch_time").unwrap();
        // token[0] = At "@" → span [0, 1)
        assert_eq!(tokens[0].kind, TokenKind::At);
        assert_eq!(tokens[0].span.start, 0);
        assert_eq!(tokens[0].span.end, 1);
        // token[1] = Ident "watch_time" → span [1, 11)
        assert_eq!(tokens[1].kind, TokenKind::Ident);
        assert_eq!(tokens[1].span.start, 1);
        assert_eq!(tokens[1].span.end, 11);
    }

    #[test]
    fn span_eof_is_len_len() {
        let src = "abc";
        let tokens = tokenize(src).unwrap();
        let eof = tokens.last().unwrap();
        assert_eq!(eof.kind, TokenKind::Eof);
        assert_eq!(eof.span.start, src.len());
        assert_eq!(eof.span.end, src.len());
    }

    #[test]
    fn span_string_token_includes_quotes() {
        // "'mobile'" is 8 bytes; span should be [0, 8)
        let tokens = tokenize("'mobile'").unwrap();
        assert_eq!(tokens[0].kind, TokenKind::String);
        assert_eq!(tokens[0].span.start, 0);
        assert_eq!(tokens[0].span.end, 8);
        assert_eq!(tokens[0].value, "'mobile'");
    }

    #[test]
    fn span_operators_after_whitespace() {
        // "  !=" — Neq spans [2, 4)
        let tokens = tokenize("  !=").unwrap();
        assert_eq!(tokens[0].kind, TokenKind::Neq);
        assert_eq!(tokens[0].span.start, 2);
        assert_eq!(tokens[0].span.end, 4);
    }

    // ── Error case tests ─────────────────────────────────────────────────────

    #[test]
    fn error_bare_exclamation() {
        let err = tokenize("!").unwrap_err();
        assert_eq!(err.span.start, 0);
        assert!(err.message.contains("expected '!='"), "message: {}", err.message);
    }

    #[test]
    fn error_bare_exclamation_mid_expression() {
        // "a ! b" — error at offset 2 (the '!')
        let err = tokenize("a ! b").unwrap_err();
        assert_eq!(err.span.start, 2);
    }

    #[test]
    fn error_unterminated_string() {
        let err = tokenize("'mobile").unwrap_err();
        assert_eq!(err.span.start, 0, "error span should point to opening quote");
        assert!(err.message.contains("unterminated"), "message: {}", err.message);
    }

    #[test]
    fn error_uppercase_character() {
        // Lock 1: identifiers are lowercase-only; 'M' should error immediately.
        let err = tokenize("Mean").unwrap_err();
        assert_eq!(err.span.start, 0);
        assert!(err.message.contains("unexpected character"), "message: {}", err.message);
    }

    #[test]
    fn error_stray_question_mark() {
        let err = tokenize("?").unwrap_err();
        assert_eq!(err.span.start, 0);
        assert!(err.message.contains("unexpected character"), "message: {}", err.message);
    }

    // ── Edge cases ───────────────────────────────────────────────────────────

    #[test]
    fn number_dot_number_is_one_token() {
        // "3.14" should be a single Number token, not "3" + "." + "14"
        let tokens = tokenize("3.14").unwrap();
        assert_eq!(tokens.len(), 2); // Number + Eof
        assert_eq!(tokens[0].kind, TokenKind::Number);
        assert_eq!(tokens[0].value, "3.14");
    }

    #[test]
    fn ident_adjacent_to_number_stops_correctly() {
        // "count(42)" — number inside parens does not bleed into surrounding tokens
        let tokens = tokenize("count(42)").unwrap();
        let kinds: Vec<TokenKind> = tokens.iter().map(|t| t.kind).collect();
        assert_eq!(
            kinds,
            vec![
                TokenKind::Keyword, // count
                TokenKind::LParen,
                TokenKind::Number,
                TokenKind::RParen,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn underscore_only_ident() {
        // "_" is a valid identifier start
        let tokens = tokenize("_private").unwrap();
        assert_eq!(tokens[0].kind, TokenKind::Ident);
        assert_eq!(tokens[0].value, "_private");
    }

    #[test]
    fn leading_underscore_not_keyword() {
        // Underscore-prefixed identifiers cannot be keywords (none start with '_')
        let tokens = tokenize("_within").unwrap();
        assert_eq!(tokens[0].kind, TokenKind::Ident);
    }

    #[test]
    fn multiline_whitespace_skipped() {
        let tokens = tokenize("mean\n(\n)\n").unwrap();
        let kinds: Vec<_> = tokens.iter().map(|t| t.kind).collect();
        assert_eq!(kinds, vec![
            TokenKind::Keyword, TokenKind::LParen, TokenKind::RParen, TokenKind::Eof
        ]);
    }
}
