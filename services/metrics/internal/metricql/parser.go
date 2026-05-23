package metricql

import (
	"fmt"
	"strconv"
)

// ParseError carries a span-tagged parser error suitable for diagnostic display.
type ParseError struct {
	Span    Span
	Message string
	Source  string
}

func (e *ParseError) Error() string {
	if e.Source != "" && e.Span.Start < len(e.Source) {
		return fmt.Sprintf("metricql parse error at offset %d: %s (near %q)",
			e.Span.Start, e.Message, snippetAround(e.Source, e.Span.Start))
	}
	return fmt.Sprintf("metricql parse error at offset %d: %s", e.Span.Start, e.Message)
}

// Parser consumes a token stream and produces a typed AST.
type Parser struct {
	tokens []Token
	pos    int
	source string
}

// NewParser constructs a parser over the given token stream.
func NewParser(tokens []Token) *Parser { return &Parser{tokens: tokens} }

// Parse lexes and parses MetricQL source, returning the root AST node.
func Parse(source string) (Node, error) {
	toks, err := NewLexer(source).Tokenize()
	if err != nil {
		return nil, err
	}
	p := &Parser{tokens: toks, source: source}
	expr, err := p.parseExpression()
	if err != nil {
		return nil, err
	}
	if p.peek().Kind != TokEOF {
		return nil, p.errAt(p.peek().Span, fmt.Sprintf("unexpected trailing tokens starting with %s", p.peek().Kind))
	}
	return expr, nil
}

// --- Token-stream helpers ---------------------------------------------------

func (p *Parser) peek() Token {
	if p.pos >= len(p.tokens) {
		// Should never happen -- lexer always appends TokEOF.
		return Token{Kind: TokEOF}
	}
	return p.tokens[p.pos]
}

func (p *Parser) peekAt(offset int) Token {
	idx := p.pos + offset
	if idx >= len(p.tokens) {
		return Token{Kind: TokEOF}
	}
	return p.tokens[idx]
}

func (p *Parser) advance() Token {
	t := p.peek()
	p.pos++
	return t
}

func (p *Parser) expect(kind TokenKind, descriptionForError string) (Token, error) {
	t := p.peek()
	if t.Kind != kind {
		return t, p.errAt(t.Span, fmt.Sprintf("expected %s, got %s", descriptionForError, t.Kind))
	}
	return p.advance(), nil
}

func (p *Parser) expectKeyword(kw string) (Token, error) {
	t := p.peek()
	if t.Kind != TokKeyword || t.Value != kw {
		return t, p.errAt(t.Span, fmt.Sprintf("expected keyword %q, got %s %q", kw, t.Kind, t.Value))
	}
	return p.advance(), nil
}

func (p *Parser) errAt(span Span, msg string) error {
	return &ParseError{Span: span, Message: msg, Source: p.source}
}

// spanFromTo returns a Span covering [start.Start, end.End).
func spanFromTo(start, end Span) Span {
	return Span{Start: start.Start, End: end.End}
}

// --- Grammar productions ----------------------------------------------------

var aggregationKeywords = map[string]struct{}{
	"mean":           {},
	"sum":            {},
	"count":          {},
	"count_distinct": {},
	"proportion":     {},
	"percentile":     {},
}

func (p *Parser) parseExpression() (Node, error) {
	t := p.peek()
	if t.Kind == TokKeyword {
		if _, ok := aggregationKeywords[t.Value]; ok {
			return p.parseAggregation()
		}
		// 'ratio' starts a composite-expr factor; anything else is a syntax error.
		if t.Value == "ratio" {
			return p.parseComposite()
		}
		return nil, p.errAt(t.Span, fmt.Sprintf("expected aggregation or composite expression, got keyword %q", t.Value))
	}
	switch t.Kind {
	case TokAt, TokLParen, TokNumber, TokMinus:
		return p.parseComposite()
	default:
		return nil, p.errAt(t.Span, fmt.Sprintf("expected aggregation or composite expression, got %s", t.Kind))
	}
}

// parseAggregation: agg_func '(' source ')' filter? window?
func (p *Parser) parseAggregation() (*Aggregation, error) {
	startTok := p.peek()
	if startTok.Kind != TokKeyword {
		return nil, p.errAt(startTok.Span, fmt.Sprintf("expected aggregation function, got %s", startTok.Kind))
	}

	agg := &Aggregation{}
	switch startTok.Value {
	case "mean":
		agg.Func = AggMean
		p.advance()
	case "sum":
		agg.Func = AggSum
		p.advance()
	case "count":
		agg.Func = AggCount
		p.advance()
	case "count_distinct":
		agg.Func = AggCountDistinct
		p.advance()
	case "proportion":
		agg.Func = AggProportion
		p.advance()
	case "percentile":
		agg.Func = AggPercentile
		p.advance()
		// 'percentile' '(' NUMBER ')'
		if _, err := p.expect(TokLParen, "'(' after 'percentile'"); err != nil {
			return nil, err
		}
		numTok := p.peek()
		if numTok.Kind != TokNumber {
			return nil, p.errAt(numTok.Span, fmt.Sprintf("expected percentile value (NUMBER), got %s", numTok.Kind))
		}
		pct, err := strconv.ParseFloat(numTok.Value, 64)
		if err != nil {
			return nil, p.errAt(numTok.Span, fmt.Sprintf("invalid percentile value %q: %v", numTok.Value, err))
		}
		if pct <= 0 || pct >= 100 {
			return nil, p.errAt(numTok.Span, fmt.Sprintf("percentile must be in (0, 100), got %v", pct))
		}
		agg.Percentile = pct
		p.advance()
		if _, err := p.expect(TokRParen, "')' after percentile value"); err != nil {
			return nil, err
		}
	default:
		return nil, p.errAt(startTok.Span, fmt.Sprintf("unknown aggregation function %q", startTok.Value))
	}

	if _, err := p.expect(TokLParen, "'(' after aggregation function"); err != nil {
		return nil, err
	}

	src, err := p.parseSource()
	if err != nil {
		return nil, err
	}
	agg.Source = src

	closeTok, err := p.expect(TokRParen, "')' after aggregation source")
	if err != nil {
		return nil, err
	}

	// Optional filter
	if t := p.peek(); t.Kind == TokKeyword && t.Value == "where" {
		filter, err := p.parseFilter()
		if err != nil {
			return nil, err
		}
		agg.Filter = &filter
	}

	// Optional window
	if t := p.peek(); t.Kind == TokKeyword && t.Value == "within" {
		win, err := p.parseWindow()
		if err != nil {
			return nil, err
		}
		agg.Window = &win
	}

	end := closeTok.Span
	if agg.Filter != nil {
		end = agg.Filter.Span()
	}
	if agg.Window != nil {
		end = agg.Window.Span()
	}
	agg.SetSpan(spanFromTo(startTok.Span, end))
	return agg, nil
}

// parseSource: event_type ( '.' field )?
func (p *Parser) parseSource() (Source, error) {
	evTok, err := p.expect(TokIdent, "event identifier")
	if err != nil {
		return Source{}, err
	}
	src := Source{EventType: evTok.Value}
	endSpan := evTok.Span
	if p.peek().Kind == TokDot {
		p.advance()
		fTok, err := p.expect(TokIdent, "field identifier after '.'")
		if err != nil {
			return Source{}, err
		}
		src.Field = fTok.Value
		endSpan = fTok.Span
	}
	return SourceWithSpan(src, spanFromTo(evTok.Span, endSpan)), nil
}

// parseFilter: 'where' predicate ('and' predicate)*
func (p *Parser) parseFilter() (Filter, error) {
	startTok, err := p.expectKeyword("where")
	if err != nil {
		return Filter{}, err
	}
	first, err := p.parsePredicate()
	if err != nil {
		return Filter{}, err
	}
	preds := []Predicate{first}
	endSpan := first.Span()
	for p.peek().Kind == TokKeyword && p.peek().Value == "and" {
		p.advance()
		next, err := p.parsePredicate()
		if err != nil {
			return Filter{}, err
		}
		preds = append(preds, next)
		endSpan = next.Span()
	}
	f := Filter{Predicates: preds}
	return FilterWithSpan(f, spanFromTo(startTok.Span, endSpan)), nil
}

// parsePredicate: field_ref operator value
func (p *Parser) parsePredicate() (Predicate, error) {
	fr, frStart, frEnd, err := p.parseFieldRef()
	if err != nil {
		return Predicate{}, err
	}
	op, err := p.parseOperator()
	if err != nil {
		return Predicate{}, err
	}
	val, err := p.parseValue()
	if err != nil {
		return Predicate{}, err
	}
	pred := Predicate{Field: fr, Operator: op, Value: val}
	_ = frEnd // used implicitly via the span we build below
	return PredicateWithSpan(pred, spanFromTo(frStart, val.Span())), nil
}

// parseFieldRef: IDENTIFIER ( '.' IDENTIFIER )?
// Returns the FieldRef plus its start/end spans for the predicate's span.
func (p *Parser) parseFieldRef() (FieldRef, Span, Span, error) {
	t1, err := p.expect(TokIdent, "field identifier")
	if err != nil {
		return FieldRef{}, Span{}, Span{}, err
	}
	if p.peek().Kind == TokDot {
		p.advance()
		t2, err := p.expect(TokIdent, "field identifier after '.'")
		if err != nil {
			return FieldRef{}, Span{}, Span{}, err
		}
		return FieldRef{Namespace: t1.Value, Name: t2.Value}, t1.Span, t2.Span, nil
	}
	return FieldRef{Name: t1.Value}, t1.Span, t1.Span, nil
}

// parseOperator: '=' | '!=' | '>' | '<' | '>=' | '<=' | 'in'
func (p *Parser) parseOperator() (Op, error) {
	t := p.peek()
	switch t.Kind {
	case TokEq:
		p.advance()
		return OpEq, nil
	case TokNeq:
		p.advance()
		return OpNeq, nil
	case TokLt:
		p.advance()
		return OpLt, nil
	case TokLte:
		p.advance()
		return OpLte, nil
	case TokGt:
		p.advance()
		return OpGt, nil
	case TokGte:
		p.advance()
		return OpGte, nil
	case TokKeyword:
		if t.Value == "in" {
			p.advance()
			return OpIn, nil
		}
	}
	return 0, p.errAt(t.Span, fmt.Sprintf("expected operator (=, !=, <, <=, >, >=, in), got %s %q", t.Kind, t.Value))
}

// parseValue: STRING | NUMBER | '[' value ( ',' value )* ']'
func (p *Parser) parseValue() (Value, error) {
	t := p.peek()
	switch t.Kind {
	case TokString:
		p.advance()
		s := stripQuotes(t.Value)
		return ValueWithSpan(Value{String: &s}, t.Span), nil
	case TokNumber:
		p.advance()
		n, err := strconv.ParseFloat(t.Value, 64)
		if err != nil {
			return Value{}, p.errAt(t.Span, fmt.Sprintf("invalid number %q: %v", t.Value, err))
		}
		return ValueWithSpan(Value{Number: &n}, t.Span), nil
	case TokLBracket:
		startTok := p.advance()
		items := []Value{}
		if p.peek().Kind == TokRBracket {
			return Value{}, p.errAt(p.peek().Span, "in-list must contain at least one value")
		}
		for {
			v, err := p.parseValue()
			if err != nil {
				return Value{}, err
			}
			items = append(items, v)
			if p.peek().Kind == TokComma {
				p.advance()
				continue
			}
			break
		}
		endTok, err := p.expect(TokRBracket, "']' to close in-list")
		if err != nil {
			return Value{}, err
		}
		return ValueWithSpan(Value{List: items}, spanFromTo(startTok.Span, endTok.Span)), nil
	}
	return Value{}, p.errAt(t.Span, fmt.Sprintf("expected value (STRING, NUMBER, or '['), got %s", t.Kind))
}

// stripQuotes removes the surrounding single quotes from a TokString value.
// The lexer never emits malformed strings, so we don't validate here.
func stripQuotes(s string) string {
	if len(s) >= 2 && s[0] == '\'' && s[len(s)-1] == '\'' {
		return s[1 : len(s)-1]
	}
	return s
}

// parseWindow: 'within' NUMBER ( 'hours' | 'days' ) 'of' 'exposure'
func (p *Parser) parseWindow() (Window, error) {
	startTok, err := p.expectKeyword("within")
	if err != nil {
		return Window{}, err
	}
	nTok, err := p.expect(TokNumber, "NUMBER after 'within'")
	if err != nil {
		return Window{}, err
	}
	nFloat, err := strconv.ParseFloat(nTok.Value, 64)
	if err != nil {
		return Window{}, p.errAt(nTok.Span, fmt.Sprintf("invalid window size %q: %v", nTok.Value, err))
	}
	if nFloat != float64(int(nFloat)) || nFloat <= 0 {
		return Window{}, p.errAt(nTok.Span, fmt.Sprintf("window size must be a positive integer, got %v", nFloat))
	}
	n := int(nFloat)

	unitTok := p.peek()
	if unitTok.Kind != TokKeyword || (unitTok.Value != "hours" && unitTok.Value != "days") {
		return Window{}, p.errAt(unitTok.Span, fmt.Sprintf("expected 'hours' or 'days', got %s %q", unitTok.Kind, unitTok.Value))
	}
	p.advance()
	var unit WindowUnit
	if unitTok.Value == "hours" {
		unit = WindowHours
	} else {
		unit = WindowDays
	}

	if _, err := p.expectKeyword("of"); err != nil {
		return Window{}, err
	}
	endTok, err := p.expectKeyword("exposure")
	if err != nil {
		return Window{}, err
	}

	w := Window{N: n, Unit: unit}
	return WindowWithSpan(w, spanFromTo(startTok.Span, endTok.Span)), nil
}

// --- Composite-expression chain (with precedence) ---------------------------

// parseComposite: term ( ( '+' | '-' ) term )*
func (p *Parser) parseComposite() (Node, error) {
	left, err := p.parseTerm()
	if err != nil {
		return nil, err
	}
	for {
		t := p.peek()
		if t.Kind != TokPlus && t.Kind != TokMinus {
			return left, nil
		}
		p.advance()
		right, err := p.parseTerm()
		if err != nil {
			return nil, err
		}
		op := OpAdd
		if t.Kind == TokMinus {
			op = OpSub
		}
		c := &Composite{Op: op, Left: left, Right: right}
		c.SetSpan(spanFromTo(left.Span(), right.Span()))
		left = c
	}
}

// parseTerm: unary ( ( '*' | '/' ) unary )*
func (p *Parser) parseTerm() (Node, error) {
	left, err := p.parseUnary()
	if err != nil {
		return nil, err
	}
	for {
		t := p.peek()
		if t.Kind != TokStar && t.Kind != TokSlash {
			return left, nil
		}
		p.advance()
		right, err := p.parseUnary()
		if err != nil {
			return nil, err
		}
		op := OpMul
		if t.Kind == TokSlash {
			op = OpDiv
		}
		c := &Composite{Op: op, Left: left, Right: right}
		c.SetSpan(spanFromTo(left.Span(), right.Span()))
		left = c
	}
}

// parseUnary: '-'? factor
//
// This is the ONLY place '-' becomes negation. Every other '-' in the
// grammar is binary subtraction handled in parseComposite. Round-6 review
// constraint -- do not produce Negate from anywhere else.
func (p *Parser) parseUnary() (Node, error) {
	if p.peek().Kind == TokMinus {
		minusTok := p.advance()
		operand, err := p.parseFactor()
		if err != nil {
			return nil, err
		}
		n := &Negate{Operand: operand}
		n.SetSpan(spanFromTo(minusTok.Span, operand.Span()))
		return n, nil
	}
	return p.parseFactor()
}

// parseFactor: metric_ref | NUMBER | '(' composite_expr ')' | ratio_expr
func (p *Parser) parseFactor() (Node, error) {
	t := p.peek()
	switch t.Kind {
	case TokAt:
		return p.parseMetricRef()
	case TokNumber:
		p.advance()
		v, err := strconv.ParseFloat(t.Value, 64)
		if err != nil {
			return nil, p.errAt(t.Span, fmt.Sprintf("invalid number %q: %v", t.Value, err))
		}
		lit := &Literal{Value: v}
		lit.SetSpan(t.Span)
		return lit, nil
	case TokLParen:
		startTok := p.advance()
		inner, err := p.parseComposite()
		if err != nil {
			return nil, err
		}
		endTok, err := p.expect(TokRParen, "')' to close parenthesized expression")
		if err != nil {
			return nil, err
		}
		// Re-stamp the span to cover the parens for accurate error highlighting.
		switch n := inner.(type) {
		case *Composite:
			n.SetSpan(spanFromTo(startTok.Span, endTok.Span))
		case *Negate:
			n.SetSpan(spanFromTo(startTok.Span, endTok.Span))
		case *Literal:
			n.SetSpan(spanFromTo(startTok.Span, endTok.Span))
		case *MetricRef:
			n.SetSpan(spanFromTo(startTok.Span, endTok.Span))
		case *Ratio:
			n.SetSpan(spanFromTo(startTok.Span, endTok.Span))
		}
		return inner, nil
	case TokKeyword:
		if t.Value == "ratio" {
			return p.parseRatio()
		}
		return nil, p.errAt(t.Span, fmt.Sprintf("expected metric_ref, NUMBER, '(', or 'ratio', got keyword %q", t.Value))
	}
	return nil, p.errAt(t.Span, fmt.Sprintf("expected metric_ref, NUMBER, '(', or 'ratio', got %s", t.Kind))
}

// parseMetricRef: '@' IDENTIFIER
func (p *Parser) parseMetricRef() (*MetricRef, error) {
	atTok, err := p.expect(TokAt, "'@'")
	if err != nil {
		return nil, err
	}
	idTok, err := p.expect(TokIdent, "metric identifier after '@'")
	if err != nil {
		return nil, err
	}
	m := &MetricRef{ID: idTok.Value}
	m.SetSpan(spanFromTo(atTok.Span, idTok.Span))
	return m, nil
}

// parseRatio: 'ratio' '(' metric_ref ',' metric_ref ')'
func (p *Parser) parseRatio() (*Ratio, error) {
	startTok, err := p.expectKeyword("ratio")
	if err != nil {
		return nil, err
	}
	if _, err := p.expect(TokLParen, "'(' after 'ratio'"); err != nil {
		return nil, err
	}
	num, err := p.parseMetricRef()
	if err != nil {
		return nil, err
	}
	if _, err := p.expect(TokComma, "',' between ratio arguments"); err != nil {
		return nil, err
	}
	den, err := p.parseMetricRef()
	if err != nil {
		return nil, err
	}
	endTok, err := p.expect(TokRParen, "')' to close ratio")
	if err != nil {
		return nil, err
	}
	r := &Ratio{Numerator: *num, Denominator: *den}
	r.SetSpan(spanFromTo(startTok.Span, endTok.Span))
	return r, nil
}
