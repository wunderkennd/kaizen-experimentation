//! MetricQL recursive-descent parser — direct port of
//! `services/metrics/internal/metricql/parser.go` (ADR-026 Phase 2 / #436).
//!
//! ## Entry points
//!
//! - [`parse`]        — lex + parse a source string in one call (preferred).
//! - [`parse_tokens`] — parse a pre-lexed token stream.
//!
//! ## Grammar (summary)
//!
//! ```text
//! expression     = aggregation | composite
//! aggregation    = agg_func '(' source ')' filter? window?
//! agg_func       = 'mean' | 'sum' | 'count' | 'count_distinct' | 'proportion'
//!                | 'percentile' '(' NUMBER ')'
//! source         = IDENT ( '.' IDENT )?
//! filter         = 'where' predicate ( 'and' predicate )*
//! predicate      = field_ref operator value
//! field_ref      = IDENT ( '.' IDENT )?
//! operator       = '=' | '!=' | '<' | '<=' | '>' | '>=' | 'in'
//! value          = STRING | NUMBER | '[' value ( ',' value )* ']'
//! window         = 'within' NUMBER ( 'hours' | 'days' ) 'of' 'exposure'
//! composite      = term ( ( '+' | '-' ) term )*
//! term           = unary ( ( '*' | '/' ) unary )*
//! unary          = '-'? factor
//! factor         = metric_ref | NUMBER | '(' composite ')' | ratio
//! metric_ref     = '@' IDENT
//! ratio          = 'ratio' '(' metric_ref ',' metric_ref ')'
//! ```
//!
//! ## Critical invariants (Lock 1 / Round-6 review)
//!
//! 1. `parse_unary` is the **only** place `Minus` becomes `Negate`. Every other
//!    `-` in the grammar is binary subtraction in `parse_composite`.
//! 2. `percentile(N)` enforces `0 < N < 100` at parse time.
//! 3. Window `N` must be a positive integer at parse time.
//! 4. Parens re-stamp the inner node's span to cover `[lp, rp]`.

use super::ast::{
    AggFunc, Aggregation, ArithOp, Composite, FieldRef, Filter, Literal, MetricRef, Negate, Node,
    Op, Predicate, Ratio, Source, Span, Value, ValueKind, Window, WindowUnit,
};
use super::lexer::{tokenize, LexError, Token, TokenKind};

// ---------------------------------------------------------------------------
// ParseError
// ---------------------------------------------------------------------------

/// A span-tagged parse error. Mirrors Go `ParseError{Span, Message, Source}`.
///
/// If the error originates in the lex phase, the same `Span` and `message`
/// from the [`LexError`] are forwarded — no separate error type.
#[derive(Debug, Clone, PartialEq)]
pub struct ParseError {
    pub span: Span,
    pub message: String,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "metricql parse error at offset {}: {}", self.span.start, self.message)
    }
}

impl std::error::Error for ParseError {}

impl From<LexError> for ParseError {
    fn from(e: LexError) -> Self {
        ParseError { span: e.span, message: e.message }
    }
}

// ---------------------------------------------------------------------------
// Public entry points
// ---------------------------------------------------------------------------

/// Lex and parse a MetricQL source string, returning the root AST node.
///
/// Mirrors Go's `Parse(source string) (Node, error)`.
/// Any [`LexError`] is converted to a [`ParseError`] with the same span/message.
pub fn parse(source: &str) -> Result<Node, ParseError> {
    let tokens = tokenize(source).map_err(ParseError::from)?;
    let mut p = Parser::new(tokens);
    let expr = p.parse_expression()?;
    let next = p.peek();
    if next.kind != TokenKind::Eof {
        return Err(p.err_at(
            next.span.clone(),
            &format!("unexpected trailing tokens starting with {:?}", next.kind),
        ));
    }
    Ok(expr)
}

/// Parse a pre-lexed token stream.
///
/// Mirrors Go's `NewParser(tokens).parseExpression()` pattern used by tests
/// that construct their own token streams.
pub fn parse_tokens(tokens: Vec<Token>) -> Result<Node, ParseError> {
    let mut p = Parser::new(tokens);
    let expr = p.parse_expression()?;
    let next = p.peek();
    if next.kind != TokenKind::Eof {
        return Err(p.err_at(
            next.span.clone(),
            &format!("unexpected trailing tokens starting with {:?}", next.kind),
        ));
    }
    Ok(expr)
}

// ---------------------------------------------------------------------------
// Parser struct (private)
// ---------------------------------------------------------------------------

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    // ── Token-stream helpers ─────────────────────────────────────────────────

    fn peek(&self) -> &Token {
        if self.pos < self.tokens.len() {
            &self.tokens[self.pos]
        } else {
            // Lexer always appends Eof; this branch is unreachable in practice.
            self.tokens.last().expect("token stream must not be empty")
        }
    }

    #[allow(dead_code)] // Mirrors Go's peekAt; used by A4 semantic analyser.
    fn peek_at(&self, offset: usize) -> &Token {
        let idx = self.pos + offset;
        if idx < self.tokens.len() {
            &self.tokens[idx]
        } else {
            self.tokens.last().expect("token stream must not be empty")
        }
    }

    fn advance(&mut self) -> Token {
        let t = self.tokens[self.pos].clone();
        self.pos += 1;
        t
    }

    fn expect(&mut self, kind: TokenKind, description: &str) -> Result<Token, ParseError> {
        let t = self.peek().clone();
        if t.kind != kind {
            return Err(self.err_at(
                t.span.clone(),
                &format!("expected {}, got {:?}", description, t.kind),
            ));
        }
        Ok(self.advance())
    }

    fn expect_keyword(&mut self, kw: &str) -> Result<Token, ParseError> {
        let t = self.peek().clone();
        if t.kind != TokenKind::Keyword || t.value != kw {
            return Err(self.err_at(
                t.span.clone(),
                &format!("expected keyword {:?}, got {:?} {:?}", kw, t.kind, t.value),
            ));
        }
        Ok(self.advance())
    }

    fn err_at(&self, span: Span, msg: &str) -> ParseError {
        ParseError { span, message: msg.to_owned() }
    }

    // ── Span helper ──────────────────────────────────────────────────────────

    /// Returns a Span covering `[start.start, end.end)`. Mirrors Go `spanFromTo`.
    fn span_from_to(start: &Span, end: &Span) -> Span {
        Span::new(start.start, end.end)
    }

    /// Return the span of the current node for a given `Node` variant.
    fn node_span(node: &Node) -> &Span {
        match node {
            Node::Aggregation(n) => &n.span,
            Node::Composite(n) => &n.span,
            Node::Negate(n) => &n.span,
            Node::Ratio(n) => &n.span,
            Node::MetricRef(n) => &n.span,
            Node::Literal(n) => &n.span,
        }
    }

    /// Re-stamp the span of a node, covering the supplied `[start, end)` range.
    /// Used after parsing parenthesised expressions. Mirrors Go's per-type
    /// `n.SetSpan(spanFromTo(lp, rp))` calls in `parseFactor`.
    fn restamp_span(node: Node, span: Span) -> Node {
        match node {
            Node::Composite(mut b) => {
                b.span = span;
                Node::Composite(b)
            }
            Node::Negate(mut b) => {
                b.span = span;
                Node::Negate(b)
            }
            Node::Literal(mut lit) => {
                lit.span = span;
                Node::Literal(lit)
            }
            Node::MetricRef(mut mr) => {
                mr.span = span;
                Node::MetricRef(mr)
            }
            Node::Ratio(mut r) => {
                r.span = span;
                Node::Ratio(r)
            }
            // Aggregation and bare nodes: return as-is (Go does the same set).
            other => other,
        }
    }

    // ── Grammar productions ──────────────────────────────────────────────────

    /// Top-level dispatch: aggregation if leading keyword is an agg function;
    /// composite-expr if leading token is `@ | ( | Number | Minus | 'ratio'`.
    ///
    /// Mirrors Go `parseExpression`.
    fn parse_expression(&mut self) -> Result<Node, ParseError> {
        let t = self.peek().clone();
        if t.kind == TokenKind::Keyword {
            match t.value.as_str() {
                "mean" | "sum" | "count" | "count_distinct" | "proportion" | "percentile" => {
                    return Ok(Node::Aggregation(self.parse_aggregation()?));
                }
                "ratio" => {
                    // 'ratio' starts a composite-expr factor.
                    return self.parse_composite();
                }
                other => {
                    return Err(self.err_at(
                        t.span,
                        &format!(
                            "expected aggregation or composite expression, got keyword {:?}",
                            other
                        ),
                    ));
                }
            }
        }
        match t.kind {
            TokenKind::At | TokenKind::LParen | TokenKind::Number | TokenKind::Minus => {
                self.parse_composite()
            }
            _ => Err(self.err_at(
                t.span,
                &format!(
                    "expected aggregation or composite expression, got {:?}",
                    t.kind
                ),
            )),
        }
    }

    /// `agg_func '(' source ')' filter? window?`
    ///
    /// For `percentile`: `'percentile' '(' NUMBER ')' '(' source ')' filter? window?`
    /// Enforces `0 < N < 100` at parse time (invariant 2).
    ///
    /// Mirrors Go `parseAggregation`.
    fn parse_aggregation(&mut self) -> Result<Aggregation, ParseError> {
        let start_tok = self.peek().clone();
        if start_tok.kind != TokenKind::Keyword {
            return Err(self.err_at(
                start_tok.span,
                &format!("expected aggregation function, got {:?}", start_tok.kind),
            ));
        }

        let func: AggFunc;
        let mut percentile: f64 = 0.0;

        match start_tok.value.as_str() {
            "mean" => {
                func = AggFunc::Mean;
                self.advance();
            }
            "sum" => {
                func = AggFunc::Sum;
                self.advance();
            }
            "count" => {
                func = AggFunc::Count;
                self.advance();
            }
            "count_distinct" => {
                func = AggFunc::CountDistinct;
                self.advance();
            }
            "proportion" => {
                func = AggFunc::Proportion;
                self.advance();
            }
            "percentile" => {
                func = AggFunc::Percentile;
                self.advance();
                // 'percentile' '(' NUMBER ')'
                self.expect(TokenKind::LParen, "'(' after 'percentile'")?;
                let num_tok = self.peek().clone();
                if num_tok.kind != TokenKind::Number {
                    return Err(self.err_at(
                        num_tok.span,
                        &format!(
                            "expected percentile value (NUMBER), got {:?}",
                            num_tok.kind
                        ),
                    ));
                }
                let pct: f64 =
                    num_tok.value.parse().map_err(|_| {
                        self.err_at(
                            num_tok.span.clone(),
                            &format!("invalid percentile value {:?}", num_tok.value),
                        )
                    })?;
                if pct <= 0.0 || pct >= 100.0 {
                    return Err(self.err_at(
                        num_tok.span,
                        &format!("percentile must be in (0, 100), got {}", pct),
                    ));
                }
                percentile = pct;
                self.advance();
                self.expect(TokenKind::RParen, "')' after percentile value")?;
            }
            other => {
                return Err(self.err_at(
                    start_tok.span,
                    &format!("unknown aggregation function {:?}", other),
                ));
            }
        }

        self.expect(TokenKind::LParen, "'(' after aggregation function")?;
        let source = self.parse_source()?;
        let close_tok = self.expect(TokenKind::RParen, "')' after aggregation source")?;

        // Optional filter
        let filter = if self.peek().kind == TokenKind::Keyword && self.peek().value == "where" {
            Some(self.parse_filter()?)
        } else {
            None
        };

        // Optional window
        let window = if self.peek().kind == TokenKind::Keyword && self.peek().value == "within" {
            Some(self.parse_window()?)
        } else {
            None
        };

        // Build the overall span: start → last non-None tail.
        let end_span = if let Some(w) = &window {
            w.span.clone()
        } else if let Some(f) = &filter {
            f.span.clone()
        } else {
            close_tok.span.clone()
        };
        let span = Self::span_from_to(&start_tok.span, &end_span);

        Ok(Aggregation { func, percentile, source, filter, window, span })
    }

    /// `event_type ( '.' field )?`
    ///
    /// Mirrors Go `parseSource`.
    fn parse_source(&mut self) -> Result<Source, ParseError> {
        let ev_tok = self.expect(TokenKind::Ident, "event identifier")?;
        let mut end_span = ev_tok.span.clone();
        let mut field = String::new();

        if self.peek().kind == TokenKind::Dot {
            self.advance(); // consume '.'
            let f_tok = self.expect(TokenKind::Ident, "field identifier after '.'")?;
            field = f_tok.value.clone();
            end_span = f_tok.span.clone();
        }

        let span = Self::span_from_to(&ev_tok.span, &end_span);
        Ok(Source { event_type: ev_tok.value, field, span })
    }

    /// `'where' predicate ( 'and' predicate )*`
    ///
    /// Mirrors Go `parseFilter`.
    fn parse_filter(&mut self) -> Result<Filter, ParseError> {
        let start_tok = self.expect_keyword("where")?;
        let first = self.parse_predicate()?;
        let mut end_span = first.span.clone();
        let mut predicates = vec![first];

        while self.peek().kind == TokenKind::Keyword && self.peek().value == "and" {
            self.advance(); // consume 'and'
            let next = self.parse_predicate()?;
            end_span = next.span.clone();
            predicates.push(next);
        }

        let span = Self::span_from_to(&start_tok.span, &end_span);
        Ok(Filter { predicates, span })
    }

    /// `field_ref operator value`
    ///
    /// Mirrors Go `parsePredicate`.
    fn parse_predicate(&mut self) -> Result<Predicate, ParseError> {
        let (field, fr_start, _fr_end) = self.parse_field_ref()?;
        let operator = self.parse_operator()?;
        let value = self.parse_value()?;
        let span = Self::span_from_to(&fr_start, &value.span);
        Ok(Predicate { field, operator, value, span })
    }

    /// `IDENT ( '.' IDENT )?`
    ///
    /// Returns `(FieldRef, start_span, end_span)`.
    /// Mirrors Go `parseFieldRef`.
    fn parse_field_ref(&mut self) -> Result<(FieldRef, Span, Span), ParseError> {
        let t1 = self.expect(TokenKind::Ident, "field identifier")?;
        if self.peek().kind == TokenKind::Dot {
            self.advance(); // consume '.'
            let t2 = self.expect(TokenKind::Ident, "field identifier after '.'")?;
            let fr = FieldRef { namespace: t1.value, name: t2.value };
            Ok((fr, t1.span, t2.span))
        } else {
            let fr = FieldRef { namespace: String::new(), name: t1.value };
            Ok((fr, t1.span.clone(), t1.span))
        }
    }

    /// `'=' | '!=' | '<' | '<=' | '>' | '>=' | 'in'`
    ///
    /// Mirrors Go `parseOperator`.
    fn parse_operator(&mut self) -> Result<Op, ParseError> {
        let t = self.peek().clone();
        match t.kind {
            TokenKind::Eq => {
                self.advance();
                Ok(Op::Eq)
            }
            TokenKind::Neq => {
                self.advance();
                Ok(Op::Neq)
            }
            TokenKind::Lt => {
                self.advance();
                Ok(Op::Lt)
            }
            TokenKind::Lte => {
                self.advance();
                Ok(Op::Lte)
            }
            TokenKind::Gt => {
                self.advance();
                Ok(Op::Gt)
            }
            TokenKind::Gte => {
                self.advance();
                Ok(Op::Gte)
            }
            TokenKind::Keyword if t.value == "in" => {
                self.advance();
                Ok(Op::In)
            }
            _ => Err(self.err_at(
                t.span,
                &format!(
                    "expected operator (=, !=, <, <=, >, >=, in), got {:?} {:?}",
                    t.kind, t.value
                ),
            )),
        }
    }

    /// `STRING | NUMBER | '[' value ( ',' value )* ']'`
    ///
    /// Enforces that in-lists are non-empty (invariant from Go parser.go:356).
    /// Strips surrounding single-quotes from String tokens (invariant 5).
    ///
    /// Mirrors Go `parseValue`.
    fn parse_value(&mut self) -> Result<Value, ParseError> {
        let t = self.peek().clone();
        match t.kind {
            TokenKind::String => {
                self.advance();
                // Invariant 5: strip surrounding single quotes.
                let inner = if t.value.len() >= 2
                    && t.value.starts_with('\'')
                    && t.value.ends_with('\'')
                {
                    t.value[1..t.value.len() - 1].to_owned()
                } else {
                    t.value.clone()
                };
                Ok(Value { kind: ValueKind::String(inner), span: t.span })
            }
            TokenKind::Number => {
                self.advance();
                let n: f64 = t.value.parse().map_err(|_| {
                    self.err_at(
                        t.span.clone(),
                        &format!("invalid number {:?}", t.value),
                    )
                })?;
                Ok(Value { kind: ValueKind::Number(n), span: t.span })
            }
            TokenKind::LBracket => {
                let start_tok = self.advance(); // consume '['
                let first_t = self.peek().clone();
                if first_t.kind == TokenKind::RBracket {
                    return Err(self.err_at(
                        first_t.span,
                        "in-list must contain at least one value",
                    ));
                }
                let mut items = Vec::new();
                loop {
                    items.push(self.parse_value()?);
                    if self.peek().kind == TokenKind::Comma {
                        self.advance(); // consume ','
                    } else {
                        break;
                    }
                }
                let end_tok = self.expect(TokenKind::RBracket, "']' to close in-list")?;
                let span = Self::span_from_to(&start_tok.span, &end_tok.span);
                Ok(Value { kind: ValueKind::List(items), span })
            }
            _ => Err(self.err_at(
                t.span,
                &format!(
                    "expected value (STRING, NUMBER, or '['), got {:?}",
                    t.kind
                ),
            )),
        }
    }

    /// `'within' NUMBER ( 'hours' | 'days' ) 'of' 'exposure'`
    ///
    /// Enforces N is a positive integer at parse time (invariant 3).
    ///
    /// Mirrors Go `parseWindow`.
    fn parse_window(&mut self) -> Result<Window, ParseError> {
        let start_tok = self.expect_keyword("within")?;
        let n_tok = self.expect(TokenKind::Number, "NUMBER after 'within'")?;
        let n_float: f64 = n_tok.value.parse().map_err(|_| {
            self.err_at(
                n_tok.span.clone(),
                &format!("invalid window size {:?}", n_tok.value),
            )
        })?;
        // Must be a positive integer — reject 1.5, 0, or negative values.
        if n_float != n_float.trunc() || n_float <= 0.0 {
            return Err(self.err_at(
                n_tok.span,
                &format!(
                    "window size must be a positive integer, got {}",
                    n_float
                ),
            ));
        }
        let n = n_float as u32;

        let unit_tok = self.peek().clone();
        if unit_tok.kind != TokenKind::Keyword
            || (unit_tok.value != "hours" && unit_tok.value != "days")
        {
            return Err(self.err_at(
                unit_tok.span,
                &format!(
                    "expected 'hours' or 'days', got {:?} {:?}",
                    unit_tok.kind, unit_tok.value
                ),
            ));
        }
        self.advance();
        let unit = if unit_tok.value == "hours" { WindowUnit::Hours } else { WindowUnit::Days };

        self.expect_keyword("of")?;
        let end_tok = self.expect_keyword("exposure")?;

        let span = Self::span_from_to(&start_tok.span, &end_tok.span);
        Ok(Window { n, unit, span })
    }

    // ── Composite-expression chain (with precedence) ─────────────────────────

    /// `term ( ( '+' | '-' ) term )*`  — left-associative.
    ///
    /// Mirrors Go `parseComposite`.
    fn parse_composite(&mut self) -> Result<Node, ParseError> {
        let mut left = self.parse_term()?;
        loop {
            let t = self.peek().clone();
            if t.kind != TokenKind::Plus && t.kind != TokenKind::Minus {
                return Ok(left);
            }
            self.advance();
            let right = self.parse_term()?;
            let op = if t.kind == TokenKind::Plus { ArithOp::Add } else { ArithOp::Sub };
            let span = Self::span_from_to(Self::node_span(&left), Self::node_span(&right));
            left = Node::Composite(Box::new(Composite {
                op,
                left: Box::new(left),
                right: Box::new(right),
                span,
            }));
        }
    }

    /// `unary ( ( '*' | '/' ) unary )*`  — left-associative.
    ///
    /// Mirrors Go `parseTerm`.
    fn parse_term(&mut self) -> Result<Node, ParseError> {
        let mut left = self.parse_unary()?;
        loop {
            let t = self.peek().clone();
            if t.kind != TokenKind::Star && t.kind != TokenKind::Slash {
                return Ok(left);
            }
            self.advance();
            let right = self.parse_unary()?;
            let op = if t.kind == TokenKind::Star { ArithOp::Mul } else { ArithOp::Div };
            let span = Self::span_from_to(Self::node_span(&left), Self::node_span(&right));
            left = Node::Composite(Box::new(Composite {
                op,
                left: Box::new(left),
                right: Box::new(right),
                span,
            }));
        }
    }

    /// `'-'? factor`
    ///
    /// **Lock 1 / Round-6 invariant**: this is the ONLY place `Minus` becomes
    /// `Negate`. Every other `-` is binary subtraction in `parse_composite`.
    ///
    /// Mirrors Go `parseUnary`.
    fn parse_unary(&mut self) -> Result<Node, ParseError> {
        if self.peek().kind == TokenKind::Minus {
            let minus_tok = self.advance();
            let operand = self.parse_factor()?;
            let span = Self::span_from_to(&minus_tok.span, Self::node_span(&operand));
            return Ok(Node::Negate(Box::new(Negate { operand, span })));
        }
        self.parse_factor()
    }

    /// `metric_ref | NUMBER | '(' composite ')' | ratio`
    ///
    /// Invariant 4: parens re-stamp the inner node's span to cover `[lp, rp]`.
    ///
    /// Mirrors Go `parseFactor`.
    fn parse_factor(&mut self) -> Result<Node, ParseError> {
        let t = self.peek().clone();
        match t.kind {
            TokenKind::At => {
                let mr = self.parse_metric_ref()?;
                Ok(Node::MetricRef(mr))
            }
            TokenKind::Number => {
                self.advance();
                let v: f64 = t.value.parse().map_err(|_| {
                    self.err_at(
                        t.span.clone(),
                        &format!("invalid number {:?}", t.value),
                    )
                })?;
                Ok(Node::Literal(Literal { value: v, span: t.span }))
            }
            TokenKind::LParen => {
                let start_tok = self.advance(); // consume '('
                let inner = self.parse_composite()?;
                let end_tok = self.expect(TokenKind::RParen, "')' to close parenthesized expression")?;
                // Invariant 4: re-stamp inner node span to cover the parens.
                let paren_span = Self::span_from_to(&start_tok.span, &end_tok.span);
                Ok(Self::restamp_span(inner, paren_span))
            }
            TokenKind::Keyword if t.value == "ratio" => {
                let r = self.parse_ratio()?;
                Ok(Node::Ratio(r))
            }
            TokenKind::Keyword => Err(self.err_at(
                t.span,
                &format!(
                    "expected metric_ref, NUMBER, '(', or 'ratio', got keyword {:?}",
                    t.value
                ),
            )),
            _ => Err(self.err_at(
                t.span,
                &format!(
                    "expected metric_ref, NUMBER, '(', or 'ratio', got {:?}",
                    t.kind
                ),
            )),
        }
    }

    /// `'@' IDENT`
    ///
    /// Mirrors Go `parseMetricRef`.
    fn parse_metric_ref(&mut self) -> Result<MetricRef, ParseError> {
        let at_tok = self.expect(TokenKind::At, "'@'")?;
        let id_tok = self.expect(TokenKind::Ident, "metric identifier after '@'")?;
        let span = Self::span_from_to(&at_tok.span, &id_tok.span);
        Ok(MetricRef { id: id_tok.value, span })
    }

    /// `'ratio' '(' metric_ref ',' metric_ref ')'`
    ///
    /// Mirrors Go `parseRatio`.
    fn parse_ratio(&mut self) -> Result<Ratio, ParseError> {
        let start_tok = self.expect_keyword("ratio")?;
        self.expect(TokenKind::LParen, "'(' after 'ratio'")?;
        let num = self.parse_metric_ref()?;
        self.expect(TokenKind::Comma, "',' between ratio arguments")?;
        let den = self.parse_metric_ref()?;
        let end_tok = self.expect(TokenKind::RParen, "')' to close ratio")?;
        let span = Self::span_from_to(&start_tok.span, &end_tok.span);
        Ok(Ratio { numerator: num, denominator: den, span })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::validators::metricql::ast::*;

    // ── Helpers ──────────────────────────────────────────────────────────────

    /// Simple test outcome discriminant.
    #[derive(Debug)]
    enum Expect {
        Ok,
        Err { span_start: usize, msg_contains: &'static str },
    }

    struct Case<'a> {
        name: &'a str,
        src: &'a str,
        expect: Expect,
    }

    fn run_cases(cases: &[Case]) {
        for c in cases {
            let result = parse(c.src);
            match &c.expect {
                Expect::Ok => {
                    assert!(
                        result.is_ok(),
                        "case {:?}: expected Ok, got Err({:?})",
                        c.name,
                        result.unwrap_err()
                    );
                }
                Expect::Err { span_start, msg_contains } => {
                    match result {
                        Err(e) => {
                            assert_eq!(
                                e.span.start, *span_start,
                                "case {:?}: expected error span_start={}, got {}",
                                c.name, span_start, e.span.start
                            );
                            assert!(
                                e.message.contains(msg_contains),
                                "case {:?}: error message {:?} does not contain {:?}",
                                c.name, e.message, msg_contains
                            );
                        }
                        Ok(node) => {
                            panic!("case {:?}: expected Err, got Ok({:?})", c.name, node);
                        }
                    }
                }
            }
        }
    }

    // ── Return the name of a Node variant (exhaustive match = enum-growth guard).

    fn node_name(n: &Node) -> &'static str {
        match n {
            Node::Aggregation(_) => "Aggregation",
            Node::Composite(_) => "Composite",
            Node::Negate(_) => "Negate",
            Node::Ratio(_) => "Ratio",
            Node::MetricRef(_) => "MetricRef",
            Node::Literal(_) => "Literal",
        }
    }

    // ── Happy-path table-driven tests ─────────────────────────────────────────

    #[test]
    fn happy_path_table() {
        let cases = [
            Case { name: "simple_aggregation", src: "mean(heartbeat.value)", expect: Expect::Ok },
            Case {
                name: "aggregation_with_filter",
                src: "count(login) where success = 'true'",
                expect: Expect::Ok,
            },
            Case {
                name: "aggregation_with_window",
                src: "count(login) within 24 hours of exposure",
                expect: Expect::Ok,
            },
            Case {
                name: "aggregation_filter_and_window",
                src: "mean(playtime.seconds) where platform = 'mobile' within 7 days of exposure",
                expect: Expect::Ok,
            },
            Case {
                name: "percentile",
                src: "percentile(95)(latency.value)",
                expect: Expect::Ok,
            },
            Case {
                name: "composite_simple",
                src: "0.7 * @watch_time + 0.3 * @ctr",
                expect: Expect::Ok,
            },
            Case {
                name: "composite_with_parens",
                src: "(0.7 + 0.3) * @watch_time",
                expect: Expect::Ok,
            },
            Case {
                name: "unary_negation",
                src: "-@watch_time + @ctr",
                expect: Expect::Ok,
            },
            Case { name: "ratio", src: "ratio(@logins, @signups)", expect: Expect::Ok },
            Case {
                name: "in_predicate",
                src: "count(event) where country in ['us', 'gb', 'ca']",
                expect: Expect::Ok,
            },
            Case {
                name: "single_ref_in_parens",
                src: "(@watch_time)",
                expect: Expect::Ok,
            },
            Case {
                name: "multiple_filter_predicates",
                src: "count(login) where a = 'x' and b > 2 and c in ['1', '2']",
                expect: Expect::Ok,
            },
        ];
        run_cases(&cases);
    }

    // ── Sad-path table-driven tests ───────────────────────────────────────────

    #[test]
    fn sad_path_table() {
        let cases = [
            Case {
                name: "unclosed_paren",
                src: "mean(heartbeat.value",
                // EOF is at offset 20; the error fires at the ')' position (EOF)
                expect: Expect::Err { span_start: 20, msg_contains: "')'" },
            },
            Case {
                name: "trailing_junk",
                src: "@a junk",
                expect: Expect::Err { span_start: 3, msg_contains: "unexpected trailing" },
            },
            Case {
                name: "percentile_out_of_range",
                src: "percentile(150)(x.y)",
                expect: Expect::Err { span_start: 11, msg_contains: "(0, 100)" },
            },
            Case {
                name: "window_non_integer",
                src: "count(e) within 1.5 hours of exposure",
                expect: Expect::Err { span_start: 16, msg_contains: "positive integer" },
            },
            Case {
                name: "window_zero",
                src: "count(e) within 0 hours of exposure",
                expect: Expect::Err { span_start: 16, msg_contains: "positive" },
            },
            Case {
                name: "unknown_agg_func",
                // 'median' is lexed as Ident (not a keyword), so parse_expression
                // rejects it at the top level: "expected aggregation or composite
                // expression, got Ident". The message contains "aggregation".
                src: "median(x.y)",
                expect: Expect::Err { span_start: 0, msg_contains: "aggregation" },
            },
            Case {
                name: "empty_in_list",
                src: "count(event) where x in []",
                expect: Expect::Err { span_start: 25, msg_contains: "at least one value" },
            },
            Case {
                name: "bare_ident_in_factor",
                src: "0.7 * watch_time",
                // 'watch_time' is a plain ident token; factor only accepts @/Number/(/ratio
                // The ident 'watch_time' starts at offset 6
                expect: Expect::Err { span_start: 6, msg_contains: "metric_ref" },
            },
        ];
        run_cases(&cases);
    }

    // ── Structural / shape tests ──────────────────────────────────────────────

    #[test]
    fn simple_aggregation_structure() {
        let node = parse("mean(heartbeat.value)").unwrap();
        assert_eq!(node_name(&node), "Aggregation");
        if let Node::Aggregation(agg) = &node {
            assert_eq!(agg.func, AggFunc::Mean);
            assert_eq!(agg.source.event_type, "heartbeat");
            assert_eq!(agg.source.field, "value");
            assert!(agg.filter.is_none());
            assert!(agg.window.is_none());
        }
    }

    #[test]
    fn aggregation_filter_structure() {
        let node = parse("count(login) where success = 'true'").unwrap();
        if let Node::Aggregation(agg) = &node {
            assert_eq!(agg.func, AggFunc::Count);
            let f = agg.filter.as_ref().unwrap();
            assert_eq!(f.predicates.len(), 1);
            let p = &f.predicates[0];
            assert_eq!(p.field.name, "success");
            assert_eq!(p.operator, Op::Eq);
            assert_eq!(p.value.kind, ValueKind::String("true".to_string()));
        } else {
            panic!("expected Aggregation");
        }
    }

    #[test]
    fn aggregation_window_structure() {
        let node = parse("count(login) within 24 hours of exposure").unwrap();
        if let Node::Aggregation(agg) = &node {
            let w = agg.window.as_ref().unwrap();
            assert_eq!(w.n, 24);
            assert_eq!(w.unit, WindowUnit::Hours);
        } else {
            panic!("expected Aggregation");
        }
    }

    #[test]
    fn percentile_structure() {
        let node = parse("percentile(95)(latency.value)").unwrap();
        if let Node::Aggregation(agg) = &node {
            assert_eq!(agg.func, AggFunc::Percentile);
            assert_eq!(agg.percentile, 95.0);
            assert_eq!(agg.source.event_type, "latency");
            assert_eq!(agg.source.field, "value");
        } else {
            panic!("expected Aggregation");
        }
    }

    #[test]
    fn unary_negation_structure() {
        // `-@watch_time + @ctr` → Composite(Add, Negate(MetricRef("watch_time")), MetricRef("ctr"))
        let node = parse("-@watch_time + @ctr").unwrap();
        assert_eq!(node_name(&node), "Composite");
        if let Node::Composite(c) = &node {
            assert_eq!(c.op, ArithOp::Add);
            assert_eq!(node_name(&c.left), "Negate");
            if let Node::Negate(neg) = c.left.as_ref() {
                assert_eq!(node_name(&neg.operand), "MetricRef");
                if let Node::MetricRef(mr) = &neg.operand {
                    assert_eq!(mr.id, "watch_time");
                }
            }
            assert_eq!(node_name(&c.right), "MetricRef");
            if let Node::MetricRef(mr) = c.right.as_ref() {
                assert_eq!(mr.id, "ctr");
            }
        }
    }

    #[test]
    fn ratio_structure() {
        let node = parse("ratio(@logins, @signups)").unwrap();
        assert_eq!(node_name(&node), "Ratio");
        if let Node::Ratio(r) = &node {
            assert_eq!(r.numerator.id, "logins");
            assert_eq!(r.denominator.id, "signups");
        }
    }

    #[test]
    fn in_predicate_structure() {
        let node = parse("count(event) where country in ['us', 'gb', 'ca']").unwrap();
        if let Node::Aggregation(agg) = &node {
            let f = agg.filter.as_ref().unwrap();
            let p = &f.predicates[0];
            assert_eq!(p.field.name, "country");
            assert_eq!(p.operator, Op::In);
            if let ValueKind::List(items) = &p.value.kind {
                assert_eq!(items.len(), 3);
                assert_eq!(items[0].kind, ValueKind::String("us".to_string()));
                assert_eq!(items[1].kind, ValueKind::String("gb".to_string()));
                assert_eq!(items[2].kind, ValueKind::String("ca".to_string()));
            } else {
                panic!("expected List value");
            }
        } else {
            panic!("expected Aggregation");
        }
    }

    #[test]
    fn single_ref_in_parens_returns_metric_ref() {
        let node = parse("(@watch_time)").unwrap();
        assert_eq!(node_name(&node), "MetricRef");
        if let Node::MetricRef(mr) = &node {
            assert_eq!(mr.id, "watch_time");
        }
    }

    #[test]
    fn multiple_filter_predicates_structure() {
        let node =
            parse("count(login) where a = 'x' and b > 2 and c in ['1', '2']").unwrap();
        if let Node::Aggregation(agg) = &node {
            let f = agg.filter.as_ref().unwrap();
            assert_eq!(f.predicates.len(), 3);
            assert_eq!(f.predicates[0].operator, Op::Eq);
            assert_eq!(f.predicates[1].operator, Op::Gt);
            assert_eq!(f.predicates[2].operator, Op::In);
        } else {
            panic!("expected Aggregation");
        }
    }

    // ── Span-stamping tests ───────────────────────────────────────────────────

    #[test]
    fn paren_span_covers_parens() {
        // `(0.5 * @x)` — 11 bytes including parens; span should be [0, 11).
        let src = "(0.5 * @x)";
        let node = parse(src).unwrap();
        // The resulting node is a Composite (after restamping covers the parens).
        assert_eq!(node_name(&node), "Composite");
        let span = match &node {
            Node::Composite(c) => &c.span,
            _ => panic!("expected Composite"),
        };
        assert_eq!(span.start, 0, "span.start should cover opening '('");
        assert_eq!(span.end, src.len(), "span.end should cover closing ')'");
    }

    #[test]
    fn metric_ref_span() {
        // `@watch_time` — 11 bytes; span [0, 11)
        let node = parse("@watch_time").unwrap();
        if let Node::MetricRef(mr) = &node {
            assert_eq!(mr.span.start, 0);
            assert_eq!(mr.span.end, 11);
        } else {
            panic!("expected MetricRef");
        }
    }

    #[test]
    fn aggregation_span_covers_full_expression() {
        // `mean(heartbeat.value)` — 21 bytes
        let node = parse("mean(heartbeat.value)").unwrap();
        if let Node::Aggregation(agg) = &node {
            assert_eq!(agg.span.start, 0);
            assert_eq!(agg.span.end, 21);
        } else {
            panic!("expected Aggregation");
        }
    }

    #[test]
    fn aggregation_with_window_span_covers_full() {
        // `count(login) within 24 hours of exposure` — 41 bytes
        let src = "count(login) within 24 hours of exposure";
        let node = parse(src).unwrap();
        if let Node::Aggregation(agg) = &node {
            assert_eq!(agg.span.start, 0);
            assert_eq!(agg.span.end, src.len());
        } else {
            panic!("expected Aggregation");
        }
    }

    // ── Agg-function keyword coverage ─────────────────────────────────────────

    #[test]
    fn all_agg_functions_parse() {
        let cases = [
            ("mean(e)", AggFunc::Mean),
            ("sum(e)", AggFunc::Sum),
            ("count(e)", AggFunc::Count),
            ("count_distinct(e)", AggFunc::CountDistinct),
            ("proportion(e)", AggFunc::Proportion),
        ];
        for (src, expected_func) in cases {
            let node = parse(src)
                .unwrap_or_else(|e| panic!("parse({:?}): {:?}", src, e));
            if let Node::Aggregation(agg) = node {
                assert_eq!(agg.func, expected_func, "for {:?}", src);
            } else {
                panic!("expected Aggregation for {:?}", src);
            }
        }
    }

    // ── parse_tokens entry point ──────────────────────────────────────────────

    #[test]
    fn parse_tokens_happy_path() {
        use crate::validators::metricql::lexer::tokenize;
        let tokens = tokenize("@watch_time").unwrap();
        let node = parse_tokens(tokens).unwrap();
        assert_eq!(node_name(&node), "MetricRef");
    }

    #[test]
    fn parse_tokens_trailing_tokens_error() {
        use crate::validators::metricql::lexer::tokenize;
        let tokens = tokenize("@a @b").unwrap();
        let err = parse_tokens(tokens).unwrap_err();
        assert!(err.message.contains("unexpected trailing"), "got: {:?}", err.message);
    }

    // ── Operator coverage ─────────────────────────────────────────────────────

    #[test]
    fn all_comparison_operators_parse() {
        let cases = [
            ("count(e) where x = 'a'", Op::Eq),
            ("count(e) where x != 'a'", Op::Neq),
            ("count(e) where x < 1", Op::Lt),
            ("count(e) where x <= 1", Op::Lte),
            ("count(e) where x > 1", Op::Gt),
            ("count(e) where x >= 1", Op::Gte),
            ("count(e) where x in ['a']", Op::In),
        ];
        for (src, expected_op) in cases {
            let node = parse(src)
                .unwrap_or_else(|e| panic!("parse({:?}): {:?}", src, e));
            if let Node::Aggregation(agg) = node {
                let f = agg.filter.unwrap();
                assert_eq!(f.predicates[0].operator, expected_op, "for {:?}", src);
            } else {
                panic!("expected Aggregation for {:?}", src);
            }
        }
    }

    // ── Days window unit ──────────────────────────────────────────────────────

    #[test]
    fn window_days_unit() {
        let node = parse("count(e) within 7 days of exposure").unwrap();
        if let Node::Aggregation(agg) = node {
            let w = agg.window.unwrap();
            assert_eq!(w.n, 7);
            assert_eq!(w.unit, WindowUnit::Days);
        } else {
            panic!("expected Aggregation");
        }
    }

    // ── Source with no field suffix ───────────────────────────────────────────

    #[test]
    fn source_no_field_suffix() {
        let node = parse("count(login)").unwrap();
        if let Node::Aggregation(agg) = node {
            assert!(agg.source.field.is_empty(), "field should be empty string");
        } else {
            panic!("expected Aggregation");
        }
    }

    // ── Nested field ref in filter ────────────────────────────────────────────

    #[test]
    fn nested_field_ref_in_filter() {
        let node = parse("count(e) where properties.platform = 'ios'").unwrap();
        if let Node::Aggregation(agg) = node {
            let p = &agg.filter.unwrap().predicates[0];
            assert_eq!(p.field.namespace, "properties");
            assert_eq!(p.field.name, "platform");
        } else {
            panic!("expected Aggregation");
        }
    }

    // ── Composite precedence: * binds tighter than + ──────────────────────────

    #[test]
    fn composite_precedence() {
        // `@a + @b * @c` should parse as `@a + (@b * @c)` (Mul before Add).
        let node = parse("@a + @b * @c").unwrap();
        if let Node::Composite(c) = &node {
            assert_eq!(c.op, ArithOp::Add, "outer op should be Add");
            assert_eq!(node_name(&c.left), "MetricRef");
            if let Node::Composite(inner) = c.right.as_ref() {
                assert_eq!(inner.op, ArithOp::Mul);
            } else {
                panic!("expected Composite(Mul) on right");
            }
        } else {
            panic!("expected Composite");
        }
    }

    // ── Number value in filter predicate ─────────────────────────────────────

    #[test]
    fn number_value_in_filter() {
        let node = parse("count(e) where age > 18").unwrap();
        if let Node::Aggregation(agg) = node {
            let p = &agg.filter.unwrap().predicates[0];
            assert_eq!(p.value.kind, ValueKind::Number(18.0));
        } else {
            panic!("expected Aggregation");
        }
    }
}
