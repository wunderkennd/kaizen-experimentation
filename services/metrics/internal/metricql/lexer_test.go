package metricql

import (
	"reflect"
	"strings"
	"testing"
)

func TestLexer_Tokenize(t *testing.T) {
	cases := []struct {
		name, src string
		want      []TokenKind
	}{
		{
			"simple agg",
			"mean(heartbeat.value)",
			[]TokenKind{TokKeyword, TokLParen, TokIdent, TokDot, TokIdent, TokRParen, TokEOF},
		},
		{
			"composite",
			"0.7 * @a + 0.3 * @b",
			[]TokenKind{TokNumber, TokStar, TokAt, TokIdent, TokPlus, TokNumber, TokStar, TokAt, TokIdent, TokEOF},
		},
		{
			"where clause",
			"mean(x) where p = 'mobile'",
			[]TokenKind{TokKeyword, TokLParen, TokIdent, TokRParen, TokKeyword, TokIdent, TokEq, TokString, TokEOF},
		},
		{
			"window",
			"count(s) within 7 days of exposure",
			[]TokenKind{TokKeyword, TokLParen, TokIdent, TokRParen, TokKeyword, TokNumber, TokKeyword, TokKeyword, TokKeyword, TokEOF},
		},
		{
			"in-list",
			"p in ['a', 'b']",
			[]TokenKind{TokIdent, TokKeyword, TokLBracket, TokString, TokComma, TokString, TokRBracket, TokEOF},
		},
		{
			"unary minus is its own token",
			"-@a",
			[]TokenKind{TokMinus, TokAt, TokIdent, TokEOF},
		},
		{
			"binary minus vs negation -- '@a - 3' lexes as @a MINUS 3, not @a NUMBER(-3)",
			"@a - 3",
			[]TokenKind{TokAt, TokIdent, TokMinus, TokNumber, TokEOF},
		},
		{
			"all comparison operators",
			"a = b != c < d <= e > f >= g",
			[]TokenKind{
				TokIdent, TokEq,
				TokIdent, TokNeq,
				TokIdent, TokLt,
				TokIdent, TokLte,
				TokIdent, TokGt,
				TokIdent, TokGte,
				TokIdent, TokEOF,
			},
		},
		{
			"ratio is a keyword",
			"ratio(@a, @b)",
			[]TokenKind{TokKeyword, TokLParen, TokAt, TokIdent, TokComma, TokAt, TokIdent, TokRParen, TokEOF},
		},
		{
			"count_distinct is a single keyword",
			"count_distinct(x)",
			[]TokenKind{TokKeyword, TokLParen, TokIdent, TokRParen, TokEOF},
		},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			toks, err := NewLexer(tc.src).Tokenize()
			if err != nil {
				t.Fatalf("unexpected lex error: %v", err)
			}
			got := make([]TokenKind, len(toks))
			for i, tk := range toks {
				got[i] = tk.Kind
			}
			if !reflect.DeepEqual(got, tc.want) {
				t.Fatalf("token kinds:\n got %v\nwant %v", got, tc.want)
			}
		})
	}
}

func TestLexer_SpansAreAccurate(t *testing.T) {
	toks, err := NewLexer("mean(x)").Tokenize()
	if err != nil {
		t.Fatalf("unexpected lex error: %v", err)
	}
	expectations := []struct {
		kind       TokenKind
		start, end int
	}{
		{TokKeyword, 0, 4},
		{TokLParen, 4, 5},
		{TokIdent, 5, 6},
		{TokRParen, 6, 7},
		{TokEOF, 7, 7},
	}
	if len(toks) != len(expectations) {
		t.Fatalf("got %d tokens, want %d: %#v", len(toks), len(expectations), toks)
	}
	for i, e := range expectations {
		if toks[i].Kind != e.kind || toks[i].Span.Start != e.start || toks[i].Span.End != e.end {
			t.Errorf("token %d: got {%v, [%d,%d)}, want {%v, [%d,%d)}",
				i, toks[i].Kind, toks[i].Span.Start, toks[i].Span.End,
				e.kind, e.start, e.end)
		}
	}
}

func TestLexer_NumberSpans(t *testing.T) {
	// "0.7" should be a single TokNumber spanning [0, 3).
	toks, err := NewLexer("0.7").Tokenize()
	if err != nil {
		t.Fatalf("unexpected lex error: %v", err)
	}
	if len(toks) != 2 {
		t.Fatalf("expected [NUMBER, EOF], got %#v", toks)
	}
	if toks[0].Kind != TokNumber || toks[0].Span != (Span{0, 3}) || toks[0].Value != "0.7" {
		t.Errorf("number token mismatch: %#v", toks[0])
	}
}

func TestLexer_FieldAccessNotMisreadAsFloat(t *testing.T) {
	// "x.y" must lex as IDENT DOT IDENT, not as IDENT NUMBER(.y).
	toks, err := NewLexer("x.y").Tokenize()
	if err != nil {
		t.Fatalf("unexpected lex error: %v", err)
	}
	wantKinds := []TokenKind{TokIdent, TokDot, TokIdent, TokEOF}
	gotKinds := make([]TokenKind, len(toks))
	for i, tk := range toks {
		gotKinds[i] = tk.Kind
	}
	if !reflect.DeepEqual(gotKinds, wantKinds) {
		t.Fatalf("x.y kinds: got %v want %v", gotKinds, wantKinds)
	}
}

func TestLexer_UnterminatedString(t *testing.T) {
	_, err := NewLexer("'oops").Tokenize()
	if err == nil {
		t.Fatal("expected lex error for unterminated string")
	}
	if !strings.Contains(err.Error(), "unterminated string") {
		t.Errorf("error message should mention unterminated string: %v", err)
	}
}

func TestLexer_RejectsUppercase(t *testing.T) {
	// IDENTIFIER := [a-z_][a-z0-9_]* -- uppercase is invalid.
	_, err := NewLexer("MEAN(x)").Tokenize()
	if err == nil {
		t.Fatal("expected lex error for uppercase identifier")
	}
}

func TestLexer_RejectsBangAlone(t *testing.T) {
	// '!' without '=' is invalid (no boolean negation in MetricQL).
	_, err := NewLexer("a ! b").Tokenize()
	if err == nil {
		t.Fatal("expected lex error for bare '!'")
	}
}

func TestLexer_KeywordValuePreserved(t *testing.T) {
	// Keyword tokens should preserve the raw source text in Value.
	toks, err := NewLexer("count_distinct").Tokenize()
	if err != nil {
		t.Fatalf("unexpected lex error: %v", err)
	}
	if toks[0].Kind != TokKeyword || toks[0].Value != "count_distinct" {
		t.Errorf("expected keyword count_distinct, got %#v", toks[0])
	}
}
