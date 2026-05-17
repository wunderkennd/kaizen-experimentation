//! Positive-allowlist parser for FILTERED_MEAN `filter_sql` (ADR-026 Phase 1, B3).
//!
//! Phase 1 deliberately avoids embedding a full SQL parser. Instead, we walk the
//! input left-to-right and classify every token against a tight whitelist:
//!
//!   * lowercase identifiers (`platform`, `duration_ms`)
//!   * numeric literals (`5000`, `-0.5`)
//!   * single-quoted string literals (`'mobile'`) — no embedded quotes
//!   * the 12 allowed operators: `=`, `!=`, `<`, `<=`, `>`, `>=`,
//!     `AND`, `OR`, `NOT`, `IN`, `IS NULL`, `IS NOT NULL`
//!   * punctuation: `(`, `)`, `,`
//!
//! Anything else — `LIKE`, `BETWEEN`, `REGEXP_LIKE`, function calls,
//! subqueries, semicolons, comments, uppercase identifiers — is rejected.
//!
//! Why an allowlist (not a blocklist like the Go-side `ValidateCustomSQL`):
//! `filter_sql` is a row-level predicate that gets concatenated verbatim into
//! a generated Spark SQL `WHERE` clause. A blocklist always lags new attack
//! shapes; the whitelist gives a closed, auditable surface for Phase 1.
//! Operators we deliberately exclude (`LIKE`, `BETWEEN`, `REGEXP_LIKE`) widen
//! the surface in ways we are not ready to defend yet (ReDoS, cost
//! amplification). They can be added later with a separate proposal.

use tonic::Status;

/// Maximum permitted `filter_sql` length. Picked to be generous for a
/// human-authored predicate while bounding worst-case tokenizer work.
const MAX_FILTER_SQL_LEN: usize = 4096;

/// True iff `s` is a bare lowercase identifier (`[a-z_][a-z0-9_]*`). Shared
/// with `validate_filtered_mean` (value_column) and B2's `event_type`.
///
/// SECURITY: lowercase-only is intentional. Spark identifiers are
/// case-insensitive by default, so accepting mixed-case here would multiply
/// the surface for collision/shadowing without buying expressiveness.
pub fn is_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(first) = chars.next() else { return false };
    if !(first.is_ascii_lowercase() || first == '_') {
        return false;
    }
    chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// Validate a FILTERED_MEAN `filter_sql` expression.
///
/// Returns `Ok(())` if every token classifies into the allowlist. Otherwise
/// returns `Status::invalid_argument` with a message identifying the offender.
#[allow(clippy::result_large_err)]
pub fn validate_filter_sql(sql: &str) -> Result<(), Box<Status>> {
    // -----------------------------------------------------------------------
    // Hard rejects (cheap pre-checks before tokenization).
    // -----------------------------------------------------------------------

    if sql.is_empty() || sql.chars().all(char::is_whitespace) {
        // SECURITY: empty filter is a programming bug (callers must guard with
        // the "use METRIC_TYPE_MEAN if no filter is needed" message). Reject
        // here as a defense-in-depth check.
        return Err(invalid("filter_sql must not be empty"));
    }

    if sql.len() > MAX_FILTER_SQL_LEN {
        return Err(invalid(format!(
            "filter_sql exceeds {} character limit",
            MAX_FILTER_SQL_LEN
        )));
    }

    if sql.contains(';') {
        // SECURITY: semicolons enable stacked statements in some SQL dialects.
        return Err(invalid("filter_sql must not contain semicolons"));
    }

    if sql.contains("--") || sql.contains("/*") || sql.contains("*/") {
        // SECURITY: comments are a known smuggling vector for blocklist-style
        // validators; we reject them outright.
        return Err(invalid("filter_sql must not contain SQL comments"));
    }

    // SECURITY: any `word(` shape implies a function call; we have no whitelist
    // of safe functions in Phase 1. Tokenize-time would also catch this but
    // catching it here gives a clearer error message.
    if contains_function_call(sql) {
        return Err(invalid("filter_sql must not contain function calls"));
    }

    // SECURITY: case-insensitive SELECT match — subqueries are not in the
    // Phase 1 allowlist and would let users escape the row-level predicate
    // context.
    if contains_select_keyword(sql) {
        return Err(invalid("filter_sql must not contain subqueries"));
    }

    // -----------------------------------------------------------------------
    // Tokenize + allowlist-check.
    // -----------------------------------------------------------------------

    let mut tokenizer = Tokenizer::new(sql);
    while let Some(token) = tokenizer.next_token()? {
        match token {
            Token::Identifier(s) => {
                // Bare identifier already passed `is_identifier` inside the
                // tokenizer (lowercase only). Nothing else to check.
                debug_assert!(is_identifier(s));
            }
            Token::Numeric
            | Token::StringLiteral
            | Token::Operator
            | Token::LParen
            | Token::RParen
            | Token::Comma => {
                // All explicitly allowlisted token kinds.
            }
        }
    }

    Ok(())
}

fn invalid(msg: impl Into<String>) -> Box<Status> {
    Box::new(Status::invalid_argument(msg.into()))
}

// ---------------------------------------------------------------------------
// Pre-tokenization scanners (case-insensitive).
// ---------------------------------------------------------------------------

/// SQL keywords that are legitimately followed by `(` in our allowlist. The
/// function-call pre-check exempts these so `country IN('US')` and
/// `NOT(condition)` (no space) are not flagged as function calls.
const KEYWORDS_FOLLOWABLE_BY_PAREN: &[&[u8]] = &[b"IN", b"NOT", b"AND", b"OR"];

/// Mask of byte indices that fall inside a single-quoted string literal.
/// Used by the pre-tokenization scanners so they don't false-positive on
/// content like `label = 'count(distinct)'` or `name = 'SELECT_v2'`.
///
/// Single-quote escape handling matches the tokenizer: a single quote always
/// terminates the string (no `''` escape and no `\'` escape in Phase 1). If
/// the input contains an unterminated string literal, the tail to end-of-input
/// is treated as in-string — the tokenizer will reject the unterminated string
/// with a clear error later.
fn string_literal_mask(sql: &str) -> Vec<bool> {
    let bytes = sql.as_bytes();
    let mut mask = vec![false; bytes.len()];
    let mut in_string = false;
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'\'' {
            // The quote itself counts as in-string (so prev_was_ident before a
            // closing quote doesn't accidentally pair with a following `(`).
            mask[i] = true;
            in_string = !in_string;
        } else if in_string {
            mask[i] = true;
        }
    }
    mask
}

/// True if `sql` contains `<word>(` where `<word>` is a run of identifier
/// chars (`[A-Za-z0-9_]+`) AND `<word>` is NOT one of the keywords that
/// legitimately precede `(` (IN, NOT, AND, OR). String-literal content is
/// skipped so `name = 'count(distinct)'` is not flagged.
fn contains_function_call(sql: &str) -> bool {
    let bytes = sql.as_bytes();
    let mask = string_literal_mask(sql);
    let mut word_start: Option<usize> = None;
    for (i, &b) in bytes.iter().enumerate() {
        if mask[i] {
            word_start = None;
            continue;
        }
        let is_ident = (b as char).is_ascii_alphanumeric() || b == b'_';
        if is_ident {
            if word_start.is_none() {
                word_start = Some(i);
            }
        } else {
            if b == b'(' {
                if let Some(start) = word_start {
                    let word = &bytes[start..i];
                    if !is_keyword_followable_by_paren(word) {
                        return true;
                    }
                }
            }
            word_start = None;
        }
    }
    false
}

fn is_keyword_followable_by_paren(word: &[u8]) -> bool {
    KEYWORDS_FOLLOWABLE_BY_PAREN
        .iter()
        .any(|kw| eq_ignore_ascii_case(word, kw))
}

fn eq_ignore_ascii_case(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b.iter())
            .all(|(x, y)| (*x as char).eq_ignore_ascii_case(&(*y as char)))
}

/// True if `sql` contains the keyword `SELECT` (case-insensitive, bounded by
/// non-identifier characters). String-literal content is skipped.
fn contains_select_keyword(sql: &str) -> bool {
    contains_keyword_ci(sql, "SELECT")
}

fn contains_keyword_ci(sql: &str, kw: &str) -> bool {
    let s = sql.as_bytes();
    let k = kw.as_bytes();
    if s.len() < k.len() {
        return false;
    }
    let mask = string_literal_mask(sql);
    'outer: for i in 0..=s.len() - k.len() {
        // Skip matches that begin inside a string literal.
        if mask[i] {
            continue;
        }
        // boundary before
        if i > 0 {
            let prev = s[i - 1];
            if (prev as char).is_ascii_alphanumeric() || prev == b'_' {
                continue;
            }
        }
        for (j, &kb) in k.iter().enumerate() {
            if mask[i + j] || !(s[i + j] as char).eq_ignore_ascii_case(&(kb as char)) {
                continue 'outer;
            }
        }
        // boundary after
        if i + k.len() < s.len() {
            let nxt = s[i + k.len()];
            if (nxt as char).is_ascii_alphanumeric() || nxt == b'_' {
                continue;
            }
        }
        return true;
    }
    false
}

// ---------------------------------------------------------------------------
// Tokenizer
// ---------------------------------------------------------------------------

/// Token kinds emitted by the allowlist tokenizer.
///
/// Only `Identifier` carries its slice (so the caller can re-check the
/// identifier shape via `debug_assert`); the other variants are tag-only
/// because their position in the stream is all the validator cares about.
#[derive(Debug)]
enum Token<'a> {
    Identifier(&'a str),
    Numeric,
    StringLiteral,
    /// Any of `=`, `!=`, `<`, `<=`, `>`, `>=`, `AND`, `OR`, `NOT`, `IN`,
    /// `IS NULL`, `IS NOT NULL`.
    Operator,
    LParen,
    RParen,
    Comma,
}

struct Tokenizer<'a> {
    src: &'a str,
    pos: usize,
}

impl<'a> Tokenizer<'a> {
    fn new(src: &'a str) -> Self {
        Self { src, pos: 0 }
    }

    fn rest(&self) -> &'a str {
        &self.src[self.pos..]
    }

    fn bump(&mut self, n: usize) {
        self.pos += n;
    }

    fn skip_whitespace(&mut self) {
        let bytes = self.src.as_bytes();
        while self.pos < bytes.len() && (bytes[self.pos] as char).is_whitespace() {
            self.pos += 1;
        }
    }

    fn next_token(&mut self) -> Result<Option<Token<'a>>, Box<Status>> {
        self.skip_whitespace();
        let rest = self.rest();
        if rest.is_empty() {
            return Ok(None);
        }

        let first = rest.as_bytes()[0];

        // Single-char punctuation
        match first {
            b'(' => {
                self.bump(1);
                return Ok(Some(Token::LParen));
            }
            b')' => {
                self.bump(1);
                return Ok(Some(Token::RParen));
            }
            b',' => {
                self.bump(1);
                return Ok(Some(Token::Comma));
            }
            _ => {}
        }

        // Operators: =, !=, <=, >=, <, >
        if let Some(consumed) = match_symbolic_operator(rest) {
            self.bump(consumed);
            return Ok(Some(Token::Operator));
        }

        // String literal: '...'  (no embedded single quotes in Phase 1)
        if first == b'\'' {
            // find closing quote
            let bytes = rest.as_bytes();
            let mut i = 1;
            while i < bytes.len() && bytes[i] != b'\'' {
                // SECURITY: bytes inside the string are not interpreted as SQL;
                // we just look for the closer. Multi-byte UTF-8 continuation
                // bytes never collide with ASCII `'`, so byte-level scanning
                // is safe.
                i += 1;
            }
            if i >= bytes.len() {
                return Err(invalid(
                    "filter_sql contains unterminated string literal",
                ));
            }
            self.bump(i + 1);
            return Ok(Some(Token::StringLiteral));
        }

        // Numeric literal: optional `-`, digits, optional `.digits`.
        // We treat `-` as a numeric prefix here rather than a subtract
        // operator because SQL FILTER predicates don't need subtraction
        // and `-0.5` as a literal is in scope.
        if first == b'-' || first.is_ascii_digit() {
            if let Some(consumed) = match_numeric(rest) {
                self.bump(consumed);
                return Ok(Some(Token::Numeric));
            }
            // Unrecognised `-` followed by non-digit → fall through to reject.
        }

        // Word: identifier OR keyword operator (case-insensitive).
        if first.is_ascii_alphabetic() || first == b'_' {
            let wlen = word_len(rest);
            let word = &rest[..wlen];

            // Keyword operators (case-insensitive).
            //   - AND, OR, NOT, IN — single-word
            //   - IS NULL / IS NOT NULL — multi-word
            let upper = word.to_ascii_uppercase();
            match upper.as_str() {
                "AND" | "OR" | "NOT" | "IN" => {
                    self.bump(wlen);
                    return Ok(Some(Token::Operator));
                }
                "IS" => {
                    // IS NULL or IS NOT NULL
                    self.bump(wlen);
                    self.skip_whitespace();
                    let after_is = self.rest();
                    let next_wlen = word_len(after_is);
                    if next_wlen == 0 {
                        return Err(invalid(
                            "filter_sql 'IS' must be followed by NULL or NOT NULL",
                        ));
                    }
                    let next_word = &after_is[..next_wlen];
                    let next_upper = next_word.to_ascii_uppercase();
                    match next_upper.as_str() {
                        "NULL" => {
                            self.bump(next_wlen);
                            return Ok(Some(Token::Operator));
                        }
                        "NOT" => {
                            self.bump(next_wlen);
                            self.skip_whitespace();
                            let after_not = self.rest();
                            let final_wlen = word_len(after_not);
                            if final_wlen == 0 {
                                return Err(invalid(
                                    "filter_sql 'IS NOT' must be followed by NULL",
                                ));
                            }
                            let final_word = &after_not[..final_wlen];
                            if final_word.eq_ignore_ascii_case("NULL") {
                                self.bump(final_wlen);
                                return Ok(Some(Token::Operator));
                            }
                            return Err(invalid(format!(
                                "filter_sql 'IS NOT' must be followed by NULL, got: {}",
                                final_word
                            )));
                        }
                        _ => {
                            return Err(invalid(format!(
                                "filter_sql 'IS' must be followed by NULL or NOT NULL, got: {}",
                                next_word
                            )));
                        }
                    }
                }
                _ => {}
            }

            // Not a keyword operator. Must therefore be an identifier
            // (lowercase only). SECURITY: rejects `LIKE`, `BETWEEN`,
            // `REGEXP_LIKE`, `EXISTS`, `XOR`, `Platform`, etc. — anything
            // that isn't in the keyword list above and isn't a valid
            // lowercase identifier falls through here.
            if is_identifier(word) {
                self.bump(wlen);
                return Ok(Some(Token::Identifier(word)));
            }
            return Err(invalid(format!(
                "filter_sql contains disallowed token: {}",
                word
            )));
        }

        // Anything else: a stray character we don't allow.
        // Compute the offending "token" (next whitespace-delimited run) for
        // a friendlier error message.
        let bytes = rest.as_bytes();
        let mut end = 0;
        while end < bytes.len() && !(bytes[end] as char).is_whitespace() {
            end += 1;
            if end >= 32 {
                break;
            }
        }
        Err(invalid(format!(
            "filter_sql contains disallowed token: {}",
            &rest[..end]
        )))
    }
}

fn match_symbolic_operator(rest: &str) -> Option<usize> {
    let b = rest.as_bytes();
    // 2-char first (longest match)
    if b.len() >= 2 {
        match (b[0], b[1]) {
            (b'!', b'=') | (b'<', b'=') | (b'>', b'=') => return Some(2),
            _ => {}
        }
    }
    if !b.is_empty() {
        match b[0] {
            b'=' | b'<' | b'>' => return Some(1),
            _ => {}
        }
    }
    None
}

fn match_numeric(rest: &str) -> Option<usize> {
    let b = rest.as_bytes();
    let mut i = 0;
    if b[0] == b'-' {
        i = 1;
        if i >= b.len() || !b[i].is_ascii_digit() {
            return None;
        }
    }
    let digit_start = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    if i == digit_start {
        return None;
    }
    // optional fractional
    if i < b.len() && b[i] == b'.' {
        let frac_start = i + 1;
        i = frac_start;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i == frac_start {
            // `.` not followed by a digit → not a numeric we accept
            return None;
        }
    }
    Some(i)
}

/// Length of the leading `[A-Za-z0-9_]+` run in `rest`. Returns 0 if `rest`
/// does not start with such a char.
fn word_len(rest: &str) -> usize {
    let b = rest.as_bytes();
    let mut i = 0;
    while i < b.len() && ((b[i] as char).is_ascii_alphanumeric() || b[i] == b'_') {
        i += 1;
    }
    i
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- is_identifier -------------------------------------------------------

    #[test]
    fn ident_accepts_lowercase() {
        assert!(is_identifier("platform"));
        assert!(is_identifier("duration_ms"));
        assert!(is_identifier("_internal"));
        assert!(is_identifier("a"));
        assert!(is_identifier("a1"));
    }

    #[test]
    fn ident_rejects_uppercase_or_empty() {
        assert!(!is_identifier(""));
        assert!(!is_identifier("Platform"));
        assert!(!is_identifier("PLATFORM"));
        assert!(!is_identifier("1abc"));
        assert!(!is_identifier("a-b"));
        assert!(!is_identifier("a.b"));
    }

    // -- accept cases --------------------------------------------------------

    #[test]
    fn accepts_simple_eq_string() {
        assert!(validate_filter_sql("platform = 'mobile'").is_ok());
    }

    #[test]
    fn accepts_simple_gt_number() {
        assert!(validate_filter_sql("duration_ms > 5000").is_ok());
    }

    #[test]
    fn accepts_and_combination() {
        assert!(
            validate_filter_sql("platform = 'mobile' AND duration_ms > 5000").is_ok()
        );
    }

    #[test]
    fn accepts_in_list() {
        assert!(validate_filter_sql("country IN ('US', 'CA', 'UK')").is_ok());
    }

    #[test]
    fn accepts_not_and_gte() {
        assert!(
            validate_filter_sql("engagement_score >= 0.5 AND NOT churn_flag").is_ok()
        );
    }

    #[test]
    fn accepts_is_not_null() {
        assert!(validate_filter_sql("last_login IS NOT NULL").is_ok());
    }

    #[test]
    fn accepts_is_null_or_lt() {
        assert!(validate_filter_sql("score IS NULL OR score < 0.1").is_ok());
    }

    #[test]
    fn accepts_mixed_case_keywords() {
        // Keywords (AND/OR/NOT/IN/IS/NULL/NOT) are case-insensitive.
        // Identifiers (platform, duration_ms) remain lowercase.
        assert!(
            validate_filter_sql("platform = 'mobile' aNd duration_ms > 5000").is_ok()
        );
    }

    #[test]
    fn accepts_negative_number() {
        assert!(validate_filter_sql("score > -0.5").is_ok());
    }

    #[test]
    fn accepts_le_and_ne() {
        assert!(validate_filter_sql("score <= 1 AND status != 'banned'").is_ok());
    }

    // -- reject: hard pre-checks --------------------------------------------

    #[test]
    fn rejects_semicolon() {
        let err = validate_filter_sql("platform = 'mobile';").unwrap_err();
        assert!(err.message().contains("semicolon"));
    }

    #[test]
    fn rejects_function_call() {
        let err = validate_filter_sql("LOWER(country) = 'us'").unwrap_err();
        assert!(err.message().contains("function call"));
    }

    #[test]
    fn rejects_subquery() {
        let err = validate_filter_sql("user_id IN (SELECT id FROM users)").unwrap_err();
        assert!(err.message().contains("subqueries"));
    }

    #[test]
    fn rejects_line_comment() {
        let err = validate_filter_sql("country = 'US' -- nope").unwrap_err();
        assert!(err.message().contains("comment"));
    }

    #[test]
    fn rejects_block_comment() {
        let err = validate_filter_sql("country = 'US' /* foo */").unwrap_err();
        assert!(err.message().contains("comment"));
    }

    #[test]
    fn rejects_empty() {
        let err = validate_filter_sql("").unwrap_err();
        assert!(err.message().contains("empty"));
    }

    #[test]
    fn rejects_whitespace_only() {
        let err = validate_filter_sql("   \t\n  ").unwrap_err();
        assert!(err.message().contains("empty"));
    }

    #[test]
    fn rejects_length_overflow() {
        // 4097 chars: build a long but otherwise-valid-looking string.
        let s = "a".repeat(4097);
        let err = validate_filter_sql(&s).unwrap_err();
        assert!(err.message().contains("4096") || err.message().contains("limit"));
    }

    // -- reject: tokenizer fall-throughs ------------------------------------

    #[test]
    fn rejects_like() {
        // LIKE is not in the keyword allowlist; it tokenizes as a word and
        // (being uppercase) fails identifier validation.
        let err = validate_filter_sql("platform LIKE 'mobile%'").unwrap_err();
        assert!(
            err.message().to_ascii_lowercase().contains("disallowed")
                || err.message().contains("LIKE"),
            "unexpected error: {}",
            err.message()
        );
    }

    #[test]
    fn rejects_between() {
        // BETWEEN tokenizes as a word that isn't in the allowlist.
        let err = validate_filter_sql("score BETWEEN 0.5 AND 1.0").unwrap_err();
        assert!(
            err.message().contains("BETWEEN")
                || err.message().to_ascii_lowercase().contains("disallowed")
        );
    }

    #[test]
    fn rejects_uppercase_identifier() {
        let err = validate_filter_sql("Platform = 'mobile'").unwrap_err();
        assert!(
            err.message().contains("Platform")
                || err.message().to_ascii_lowercase().contains("disallowed")
        );
    }

    #[test]
    fn rejects_exists_with_paren() {
        // `EXISTS(...)` (no space) trips the function-call pre-check; with a
        // space the EXISTS token reaches the word handler and fails the
        // lowercase identifier check. Either rejection is correct.
        let err = validate_filter_sql("EXISTS(1)").unwrap_err();
        assert!(err.message().contains("function call"));

        let err2 = validate_filter_sql("EXISTS (1)").unwrap_err();
        assert!(
            err2.message().to_ascii_lowercase().contains("disallowed")
                || err2.message().contains("EXISTS"),
            "unexpected: {}",
            err2.message()
        );
    }

    #[test]
    fn rejects_xor_keyword() {
        // `xor` isn't in the keyword allowlist. As a lowercase identifier it
        // would actually accept as `xor` (an identifier name) — but `true`
        // following it is also a lowercase identifier (booleans aren't a
        // recognised literal in Phase 1; they fall through as identifiers
        // too). Use a clearly disallowed shape instead: `~` is not a token.
        let err = validate_filter_sql("score ~ true").unwrap_err();
        assert!(err.message().to_ascii_lowercase().contains("disallowed"));
    }

    #[test]
    fn rejects_stray_punctuation() {
        let err = validate_filter_sql("platform = 'mobile' @ 5").unwrap_err();
        assert!(err.message().to_ascii_lowercase().contains("disallowed"));
    }

    #[test]
    fn rejects_unterminated_string() {
        let err = validate_filter_sql("platform = 'mobile").unwrap_err();
        assert!(err.message().contains("unterminated"));
    }

    #[test]
    fn rejects_regexp_like_function() {
        // REGEXP_LIKE followed by `(` is caught by function-call pre-check.
        let err = validate_filter_sql("REGEXP_LIKE(country, '^US$')").unwrap_err();
        assert!(err.message().contains("function call"));
    }

    #[test]
    fn rejects_is_without_null() {
        let err = validate_filter_sql("last_login IS something").unwrap_err();
        assert!(err.message().contains("IS"));
    }

    #[test]
    fn rejects_is_not_without_null() {
        let err = validate_filter_sql("last_login IS NOT something").unwrap_err();
        assert!(err.message().contains("NULL"));
    }

    // ── Devin BUG-0001 + BUG-0002 regression tests ──────────────────────────

    // BUG-0001: pre-check must NOT false-positive on string literals
    // containing `word(`.
    #[test]
    fn accepts_string_literal_with_paren_after_word() {
        // The `count(` inside the quoted string is not a function call.
        validate_filter_sql("category = 'count(distinct)'").unwrap();
        validate_filter_sql("name = 'rebuffer_rate(high)'").unwrap();
        validate_filter_sql("label = 'test(1)'").unwrap();
    }

    // BUG-0001: pre-check must NOT false-positive on SELECT inside a string.
    #[test]
    fn accepts_string_literal_containing_select() {
        validate_filter_sql("title = 'SELECT all'").unwrap();
    }

    // BUG-0002: `IN(` without a space is a legitimate SQL pattern; reject
    // would force users into `IN (` which is awkward.
    #[test]
    fn accepts_in_with_no_space_before_paren() {
        validate_filter_sql("country IN('US', 'CA')").unwrap();
    }

    #[test]
    fn accepts_not_with_no_space_before_paren() {
        validate_filter_sql("NOT(country = 'US')").unwrap();
    }

    #[test]
    fn accepts_and_or_with_no_space_before_paren() {
        // Less common but technically legitimate inside larger expressions.
        validate_filter_sql("a = 1 AND(b = 2)").unwrap();
        validate_filter_sql("a = 1 OR(b = 2)").unwrap();
    }

    // Negative regression: real function calls (non-keyword `word(`) still rejected.
    #[test]
    fn still_rejects_real_function_calls_after_bug_fix() {
        let err = validate_filter_sql("LOWER(country) = 'us'").unwrap_err();
        assert!(err.message().contains("function call"));
        let err = validate_filter_sql("custom_fn(x)").unwrap_err();
        assert!(err.message().contains("function call"));
    }

    // Negative regression: real subqueries outside string literals still rejected.
    #[test]
    fn still_rejects_real_subqueries_after_bug_fix() {
        let err = validate_filter_sql("user_id IN (SELECT id FROM users)").unwrap_err();
        assert!(err.message().contains("subqueries"));
    }
}
