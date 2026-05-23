package metricql

import (
	"fmt"
	"strings"
	"unicode"
)

// TokenKind enumerates the token types produced by the MetricQL lexer.
type TokenKind int

const (
	TokEOF TokenKind = iota
	TokIdent
	TokNumber
	TokString
	TokKeyword
	TokAt
	TokLParen
	TokRParen
	TokLBracket
	TokRBracket
	TokComma
	TokDot
	TokPlus
	TokMinus
	TokStar
	TokSlash
	TokEq
	TokNeq
	TokLt
	TokLte
	TokGt
	TokGte
)

// String returns the human-readable name of a TokenKind for diagnostics.
func (k TokenKind) String() string {
	switch k {
	case TokEOF:
		return "EOF"
	case TokIdent:
		return "IDENT"
	case TokNumber:
		return "NUMBER"
	case TokString:
		return "STRING"
	case TokKeyword:
		return "KEYWORD"
	case TokAt:
		return "@"
	case TokLParen:
		return "("
	case TokRParen:
		return ")"
	case TokLBracket:
		return "["
	case TokRBracket:
		return "]"
	case TokComma:
		return ","
	case TokDot:
		return "."
	case TokPlus:
		return "+"
	case TokMinus:
		return "-"
	case TokStar:
		return "*"
	case TokSlash:
		return "/"
	case TokEq:
		return "="
	case TokNeq:
		return "!="
	case TokLt:
		return "<"
	case TokLte:
		return "<="
	case TokGt:
		return ">"
	case TokGte:
		return ">="
	default:
		return fmt.Sprintf("TokenKind(%d)", int(k))
	}
}

// Token is a lexed unit of MetricQL source.
type Token struct {
	Kind  TokenKind
	Value string
	Span  Span
}

// reservedKeywords are identifiers that the lexer reclassifies from
// TokIdent to TokKeyword. They are matched case-sensitively (MetricQL
// is lowercase-only -- IDENTIFIER := [a-z_][a-z0-9_]* per Lock 1).
var reservedKeywords = map[string]struct{}{
	"where":          {},
	"and":            {},
	"in":             {},
	"within":         {},
	"of":             {},
	"exposure":       {},
	"hours":          {},
	"days":           {},
	"ratio":          {},
	"mean":           {},
	"sum":            {},
	"count":          {},
	"count_distinct": {},
	"proportion":     {},
	"percentile":     {},
}

// Lexer tokenizes MetricQL source.
type Lexer struct {
	source string
	pos    int
	tokens []Token
}

// NewLexer constructs a Lexer over the given source string.
func NewLexer(source string) *Lexer {
	return &Lexer{source: source}
}

// Tokenize runs the lexer end-to-end and returns the token stream.
// The stream always ends in a TokEOF whose Span is the zero-width range
// [len(source), len(source)).
func (l *Lexer) Tokenize() ([]Token, error) {
	for l.pos < len(l.source) {
		c := l.source[l.pos]

		// Whitespace
		if c == ' ' || c == '\t' || c == '\n' || c == '\r' {
			l.pos++
			continue
		}

		start := l.pos

		// Single-char tokens
		switch c {
		case '@':
			l.emit(TokAt, start, l.pos+1)
			l.pos++
			continue
		case '(':
			l.emit(TokLParen, start, l.pos+1)
			l.pos++
			continue
		case ')':
			l.emit(TokRParen, start, l.pos+1)
			l.pos++
			continue
		case '[':
			l.emit(TokLBracket, start, l.pos+1)
			l.pos++
			continue
		case ']':
			l.emit(TokRBracket, start, l.pos+1)
			l.pos++
			continue
		case ',':
			l.emit(TokComma, start, l.pos+1)
			l.pos++
			continue
		case '.':
			l.emit(TokDot, start, l.pos+1)
			l.pos++
			continue
		case '+':
			l.emit(TokPlus, start, l.pos+1)
			l.pos++
			continue
		case '-':
			// Unsigned NUMBER per Lock 1; minus is always its own token.
			// parseUnary in T3 is the only place '-' becomes negation.
			l.emit(TokMinus, start, l.pos+1)
			l.pos++
			continue
		case '*':
			l.emit(TokStar, start, l.pos+1)
			l.pos++
			continue
		case '/':
			l.emit(TokSlash, start, l.pos+1)
			l.pos++
			continue
		case '=':
			l.emit(TokEq, start, l.pos+1)
			l.pos++
			continue
		case '!':
			if l.pos+1 < len(l.source) && l.source[l.pos+1] == '=' {
				l.emit(TokNeq, start, l.pos+2)
				l.pos += 2
				continue
			}
			return nil, l.errAt(start, "unexpected '!' (expected '!=')")
		case '<':
			if l.pos+1 < len(l.source) && l.source[l.pos+1] == '=' {
				l.emit(TokLte, start, l.pos+2)
				l.pos += 2
			} else {
				l.emit(TokLt, start, l.pos+1)
				l.pos++
			}
			continue
		case '>':
			if l.pos+1 < len(l.source) && l.source[l.pos+1] == '=' {
				l.emit(TokGte, start, l.pos+2)
				l.pos += 2
			} else {
				l.emit(TokGt, start, l.pos+1)
				l.pos++
			}
			continue
		case '\'':
			if err := l.lexString(start); err != nil {
				return nil, err
			}
			continue
		}

		// Multi-char: NUMBER or IDENT
		if c >= '0' && c <= '9' {
			if err := l.lexNumber(start); err != nil {
				return nil, err
			}
			continue
		}
		if (c >= 'a' && c <= 'z') || c == '_' {
			l.lexIdentOrKeyword(start)
			continue
		}

		return nil, l.errAt(start, fmt.Sprintf("unexpected character %q", c))
	}

	l.emit(TokEOF, l.pos, l.pos)
	return l.tokens, nil
}

func (l *Lexer) emit(kind TokenKind, start, end int) {
	l.tokens = append(l.tokens, Token{
		Kind:  kind,
		Value: l.source[start:end],
		Span:  Span{Start: start, End: end},
	})
}

// lexNumber consumes [0-9]+ ('.' [0-9]+)? -- unsigned per Lock 1.
// A trailing '.' without fractional digits is an error (would conflict
// with TokDot for field access).
func (l *Lexer) lexNumber(start int) error {
	for l.pos < len(l.source) && l.source[l.pos] >= '0' && l.source[l.pos] <= '9' {
		l.pos++
	}
	// Optional fractional part. We require at least one digit after '.'
	// because trailing '.' is reserved for field access (heartbeat.value).
	if l.pos < len(l.source) && l.source[l.pos] == '.' {
		// Look ahead: only consume '.' if followed by a digit.
		if l.pos+1 < len(l.source) && l.source[l.pos+1] >= '0' && l.source[l.pos+1] <= '9' {
			l.pos++
			for l.pos < len(l.source) && l.source[l.pos] >= '0' && l.source[l.pos] <= '9' {
				l.pos++
			}
		}
	}
	l.emit(TokNumber, start, l.pos)
	return nil
}

// lexString consumes '\” [^']* '\” -- single-quoted with no escapes.
// Lock 1 explicitly excludes string escape sequences in v1.
func (l *Lexer) lexString(start int) error {
	l.pos++ // consume opening '
	for l.pos < len(l.source) && l.source[l.pos] != '\'' {
		// No escape handling; '\n' is a literal two-char sequence.
		l.pos++
	}
	if l.pos >= len(l.source) {
		return l.errAt(start, "unterminated string literal")
	}
	l.pos++ // consume closing '
	l.emit(TokString, start, l.pos)
	return nil
}

// lexIdentOrKeyword consumes [a-z_][a-z0-9_]* and reclassifies reserved
// words to TokKeyword. Uppercase characters are NOT admitted -- they'd
// produce a lexer error from the outer Tokenize loop.
func (l *Lexer) lexIdentOrKeyword(start int) {
	for l.pos < len(l.source) {
		c := l.source[l.pos]
		if (c >= 'a' && c <= 'z') || (c >= '0' && c <= '9') || c == '_' {
			l.pos++
			continue
		}
		break
	}
	text := l.source[start:l.pos]
	kind := TokIdent
	if _, ok := reservedKeywords[text]; ok {
		kind = TokKeyword
	}
	l.emit(kind, start, l.pos)
}

// LexError carries a span-tagged lexer error suitable for diagnostic display.
type LexError struct {
	Span    Span
	Message string
	Source  string
}

func (e *LexError) Error() string {
	if e.Span.Start < len(e.Source) {
		return fmt.Sprintf("metricql lex error at offset %d: %s (near %q)",
			e.Span.Start, e.Message, snippetAround(e.Source, e.Span.Start))
	}
	return fmt.Sprintf("metricql lex error at offset %d: %s", e.Span.Start, e.Message)
}

func (l *Lexer) errAt(pos int, msg string) error {
	return &LexError{
		Span:    Span{Start: pos, End: pos + 1},
		Message: msg,
		Source:  l.source,
	}
}

// snippetAround returns up to 10 chars of context around the given offset,
// for inline error messages. Newlines/tabs are rendered as spaces so the
// snippet stays on one line.
func snippetAround(s string, pos int) string {
	lo := pos - 5
	if lo < 0 {
		lo = 0
	}
	hi := pos + 5
	if hi > len(s) {
		hi = len(s)
	}
	out := strings.Map(func(r rune) rune {
		if unicode.IsSpace(r) {
			return ' '
		}
		return r
	}, s[lo:hi])
	return out
}
