package metricql

import (
	"bytes"
	"embed"
	"fmt"
	"sort"
	"strconv"
	"strings"
	"text/template"
)

//go:embed templates/*.sql.tmpl
var templateFS embed.FS

// templateFuncs are the same helper functions used by the Phase 1 template
// renderer in services/metrics/internal/spark/renderer.go -- kept identical
// so MetricQL templates can be moved to the central template loader later
// without surprises.
var templateFuncs = template.FuncMap{
	"last": func(i int, items interface{}) bool {
		// Identifies the last element in a range so trailing commas can be
		// suppressed in template emissions.
		switch v := items.(type) {
		case []string:
			return i == len(v)-1
		case []int:
			return i == len(v)-1
		}
		return false
	},
}

var (
	tmplAggregation = template.Must(template.New("aggregation.sql.tmpl").Funcs(templateFuncs).ParseFS(templateFS, "templates/aggregation.sql.tmpl"))
	tmplComposite   = template.Must(template.New("composite.sql.tmpl").Funcs(templateFuncs).ParseFS(templateFS, "templates/composite.sql.tmpl"))
	tmplRatio       = template.Must(template.New("ratio.sql.tmpl").Funcs(templateFuncs).ParseFS(templateFS, "templates/ratio.sql.tmpl"))
)

// CompileContext supplies parameters required to lower a MetricQL expression
// to a concrete Spark SQL statement.
type CompileContext struct {
	ExperimentID    string
	ComputationDate string
	MetricID        string // the metric being defined (NOT a referenced metric)
	KnownMetricIDs  map[string]bool
}

// CompileError is a span-tagged compilation error distinct from parse/analyze
// errors. Surfaces issues found only at lowering time (template errors,
// defense-in-depth rejections).
type CompileError struct {
	Span    Span
	Message string
}

func (e *CompileError) Error() string {
	return fmt.Sprintf("metricql compile error: %s", e.Message)
}

// Compile parses + analyzes + lowers MetricQL to a single Spark SQL statement
// that writes rows to delta.metric_summaries. Returns:
//   - sql: the lowered Spark SQL statement
//   - refs: the deduplicated set of @metric_ref dependencies (used by M3
//     scheduler for topo-order)
//   - err: parse, analyze, or compile error
func Compile(source string, ctx CompileContext) (sql string, refs []string, err error) {
	ast, err := Parse(source)
	if err != nil {
		return "", nil, err
	}
	if err := Analyze(ast, AnalyzeContext{KnownMetricIDs: ctx.KnownMetricIDs}); err != nil {
		return "", nil, err
	}
	refs = ExtractMetricRefs(ast)
	sql, err = lower(ast, ctx)
	return sql, refs, err
}

// ExtractMetricRefs walks the AST and returns the unique @metric_ref IDs
// present anywhere in the expression. The returned slice is deduplicated
// AND sorted for stable ordering -- consumers (dag.go::TopologicalOrder,
// M5 validator) treat the result as a SET, not a sequence, but sorting
// keeps golden-file output stable across runs.
func ExtractMetricRefs(root Node) []string {
	out := CollectMetricRefs(root)
	sort.Strings(out)
	return out
}

// --- Top-level lowering -----------------------------------------------------

// lower dispatches the top-level AST node to its statement-level lowering
// function. Bare *MetricRef / *Literal at top-level are caught here as
// defense-in-depth (Analyze rejects them first, but a future Analyze bug
// must not produce silent garbage).
func lower(root Node, ctx CompileContext) (string, error) {
	switch n := root.(type) {
	case *Aggregation:
		return lowerAggregation(n, ctx)
	case *Ratio:
		return lowerRatioStatement(n, ctx)
	case *Composite, *Negate:
		return lowerArithmeticStatement(root, ctx)
	case *MetricRef:
		return "", &CompileError{Span: n.Span(), Message: "top-level expression cannot be a bare metric reference"}
	case *Literal:
		return "", &CompileError{Span: n.Span(), Message: "top-level expression cannot be a bare literal"}
	}
	return "", fmt.Errorf("metricql: unknown node type %T", root)
}

// lowerAggregation emits the aggregation.sql.tmpl statement.
func lowerAggregation(a *Aggregation, ctx CompileContext) (string, error) {
	data := struct {
		ExperimentID       string
		ComputationDate    string
		MetricID           string
		SourceEventType    string
		ValueColumn        string
		AggFunc            string
		PercentileFraction string
		FilterSQL          string
		WindowHours        int
	}{
		ExperimentID:    ctx.ExperimentID,
		ComputationDate: ctx.ComputationDate,
		MetricID:        ctx.MetricID,
		SourceEventType: a.Source.EventType,
		ValueColumn:     a.Source.Field,
	}

	switch a.Func {
	case AggMean:
		data.AggFunc = "MEAN"
	case AggSum:
		data.AggFunc = "SUM"
	case AggCount:
		data.AggFunc = "COUNT"
	case AggCountDistinct:
		data.AggFunc = "COUNT_DISTINCT"
	case AggProportion:
		data.AggFunc = "PROPORTION"
	case AggPercentile:
		data.AggFunc = "PERCENTILE"
		// AST percentile is 0-100; Spark percentile_approx expects 0-1.
		// Use strconv to preserve precision (e.g., 0.5 not 5e-01).
		data.PercentileFraction = strconv.FormatFloat(a.Percentile/100.0, 'f', -1, 64)
	default:
		return "", &CompileError{Span: a.Span(), Message: fmt.Sprintf("unknown aggregation function %v", a.Func)}
	}

	if a.Filter != nil {
		filterSQL, err := compileFilter(a.Filter)
		if err != nil {
			return "", err
		}
		data.FilterSQL = filterSQL
	}

	if a.Window != nil {
		switch a.Window.Unit {
		case WindowHours:
			data.WindowHours = a.Window.N
		case WindowDays:
			data.WindowHours = a.Window.N * 24
		default:
			return "", &CompileError{Span: a.Window.Span(), Message: fmt.Sprintf("unknown window unit %v", a.Window.Unit)}
		}
	}

	var buf bytes.Buffer
	if err := tmplAggregation.Execute(&buf, data); err != nil {
		return "", &CompileError{Span: a.Span(), Message: fmt.Sprintf("aggregation template execute: %v", err)}
	}
	return buf.String(), nil
}

// lowerRatioStatement emits the ratio.sql.tmpl statement when ratio() is the
// top-level expression. Nested ratio() inside a composite is lowered via
// lowerExpr to a CASE expression that references the pivoted columns.
func lowerRatioStatement(r *Ratio, ctx CompileContext) (string, error) {
	data := struct {
		ExperimentID    string
		ComputationDate string
		MetricID        string
		NumeratorID     string
		DenominatorID   string
	}{
		ExperimentID:    ctx.ExperimentID,
		ComputationDate: ctx.ComputationDate,
		MetricID:        ctx.MetricID,
		NumeratorID:     r.Numerator.ID,
		DenominatorID:   r.Denominator.ID,
	}
	var buf bytes.Buffer
	if err := tmplRatio.Execute(&buf, data); err != nil {
		return "", &CompileError{Span: r.Span(), Message: fmt.Sprintf("ratio template execute: %v", err)}
	}
	return buf.String(), nil
}

// lowerArithmeticStatement emits composite.sql.tmpl for any top-level
// expression that mixes metric_refs, literals, arithmetic, and ratio() calls.
//
// The strategy: collect all referenced metric IDs (sorted), assign each a
// column index (m0, m1, ...), then lower the AST as a SQL expression in
// terms of those column names.
func lowerArithmeticStatement(root Node, ctx CompileContext) (string, error) {
	refs := ExtractMetricRefs(root)
	if len(refs) == 0 {
		return "", &CompileError{Span: root.Span(), Message: "arithmetic expression has no metric references"}
	}
	refColumns := make(map[string]string, len(refs))
	for i, id := range refs {
		refColumns[id] = fmt.Sprintf("m%d", i)
	}
	expr, err := lowerExpr(root, refColumns)
	if err != nil {
		return "", err
	}
	data := struct {
		ExperimentID    string
		ComputationDate string
		MetricID        string
		Operands        []string
		Expression      string
	}{
		ExperimentID:    ctx.ExperimentID,
		ComputationDate: ctx.ComputationDate,
		MetricID:        ctx.MetricID,
		Operands:        refs,
		Expression:      expr,
	}
	var buf bytes.Buffer
	if err := tmplComposite.Execute(&buf, data); err != nil {
		return "", &CompileError{Span: root.Span(), Message: fmt.Sprintf("composite template execute: %v", err)}
	}
	return buf.String(), nil
}

// --- Inner expression lowering ---------------------------------------------

// lowerExpr translates an arithmetic AST into a SQL expression fragment.
// refColumns maps @metric_ref IDs to their per-user pivoted column aliases
// (m0, m1, ...) inside the composite.sql.tmpl pivot CTE.
//
// lowerNegate (the round-6 Lock 2 sibling) lives here: it emits `(-<sub>)`
// preserving precedence whether Negate is top-level (`-@a`) or nested
// (`@a + -@b`).
func lowerExpr(n Node, refColumns map[string]string) (string, error) {
	switch v := n.(type) {
	case *Composite:
		left, err := lowerExpr(v.Left, refColumns)
		if err != nil {
			return "", err
		}
		right, err := lowerExpr(v.Right, refColumns)
		if err != nil {
			return "", err
		}
		opSQL, err := arithOpSQL(v.Op)
		if err != nil {
			return "", &CompileError{Span: v.Span(), Message: err.Error()}
		}
		// Division: NULLIF(rhs, 0) to avoid Spark divide-by-zero errors.
		if v.Op == OpDiv {
			return "(" + left + " / NULLIF(" + right + ", 0))", nil
		}
		return "(" + left + " " + opSQL + " " + right + ")", nil
	case *Negate:
		sub, err := lowerExpr(v.Operand, refColumns)
		if err != nil {
			return "", err
		}
		return "(-" + sub + ")", nil
	case *MetricRef:
		col, ok := refColumns[v.ID]
		if !ok {
			return "", &CompileError{Span: v.Span(), Message: fmt.Sprintf("metric reference @%s not assigned a column alias", v.ID)}
		}
		return col, nil
	case *Literal:
		return strconv.FormatFloat(v.Value, 'f', -1, 64), nil
	case *Ratio:
		numCol, numOK := refColumns[v.Numerator.ID]
		denCol, denOK := refColumns[v.Denominator.ID]
		if !numOK || !denOK {
			return "", &CompileError{Span: v.Span(), Message: "ratio operands not assigned column aliases"}
		}
		return "(CASE WHEN " + denCol + " = 0.0 THEN 0.0 ELSE " + numCol + " / " + denCol + " END)", nil
	}
	return "", &CompileError{Message: fmt.Sprintf("lowerExpr: unknown node type %T", n)}
}

func arithOpSQL(op ArithOp) (string, error) {
	switch op {
	case OpAdd:
		return "+", nil
	case OpSub:
		return "-", nil
	case OpMul:
		return "*", nil
	case OpDiv:
		return "/", nil
	}
	return "", fmt.Errorf("unknown arithmetic operator %v", op)
}

// --- Filter compilation -----------------------------------------------------

// compileFilter renders a Filter AST into a Spark SQL fragment for the
// WHERE clause. Predicates are joined by implicit AND (Lock 1).
//
// Identifier and value rendering is conservative -- the analyzer has already
// vetted identifier shape (regex), but we string-escape string values so a
// single-quote anywhere in the source can't smuggle SQL injection. (Lock 1
// excludes escape sequences in source strings, but the value text itself
// can contain quotes via valid lexing of `'it”s'` -- no, actually the lexer
// rejects that. Belt-and-suspenders.)
func compileFilter(f *Filter) (string, error) {
	parts := make([]string, 0, len(f.Predicates))
	for i, pred := range f.Predicates {
		col := renderFieldRef(pred.Field)
		val, err := renderValue(pred.Value)
		if err != nil {
			return "", err
		}
		var p string
		if pred.Operator == OpIn {
			p = col + " IN " + val
		} else {
			p = col + " " + opSQL(pred.Operator) + " " + val
		}
		if i == 0 {
			parts = append(parts, p)
		} else {
			parts = append(parts, "AND "+p)
		}
	}
	return strings.Join(parts, " "), nil
}

func renderFieldRef(fr FieldRef) string {
	if fr.Namespace != "" {
		return fr.Namespace + "." + fr.Name
	}
	return fr.Name
}

func opSQL(o Op) string {
	switch o {
	case OpEq:
		return "="
	case OpNeq:
		return "!="
	case OpLt:
		return "<"
	case OpLte:
		return "<="
	case OpGt:
		return ">"
	case OpGte:
		return ">="
	}
	return "?"
}

func renderValue(v Value) (string, error) {
	switch {
	case v.String != nil:
		// Escape any embedded single quotes by doubling them (SQL standard).
		return "'" + strings.ReplaceAll(*v.String, "'", "''") + "'", nil
	case v.Number != nil:
		return strconv.FormatFloat(*v.Number, 'f', -1, 64), nil
	case v.List != nil:
		items := make([]string, 0, len(v.List))
		for _, item := range v.List {
			rendered, err := renderValue(item)
			if err != nil {
				return "", err
			}
			items = append(items, rendered)
		}
		return "(" + strings.Join(items, ", ") + ")", nil
	}
	return "", &CompileError{Span: v.Span(), Message: "value has no concrete representation"}
}
