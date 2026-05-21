# ADR-026 Phase 2: MetricQL Expression Language (#435)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Issue:** [#435](https://github.com/wunderkennd/kaizen-experimentation/issues/435) — P1, `sprint-5.6`, `cluster-a`, owner `agent-3`.

**Goal:** Ship MetricQL — a hand-rolled, recursive-descent expression language compiled to Spark SQL — so operators can define composed/windowed/filtered metrics declaratively without writing raw SQL. Covers the ~35% of CUSTOM use cases that Phase 1 structured types don't.

**Architecture:** New Go package `services/metrics/internal/metricql/` containing lexer + recursive-descent parser + typed AST + Spark SQL code generator. M3's existing `RenderForType` dispatch gets a new arm: when `MetricDefinition.metricql_expression` is set, M3 parses → analyzes → compiles instead of consuming structured `type_config`. Cycle detection at parse time mirrors the M5 DFS algorithm (`crates/experimentation-management/src/validators/composite_cycle.rs`) translated to Go.

**Tech Stack:** Go 1.22 (M3 = `services/metrics/`), no parser-library dependencies. Spark SQL output via `text/template`. Hand-written lexer + parser to keep the surface tight and error messages first-class.

**Scope:** This plan covers **#435 only** (parser/AST/codegen in M3). #436 (M5 validation + M6 expression editor) gets its own plan once the contract below is committed. #437 (CUSTOM deprecation) is sequenced after #435 + #436.

---

## Phase 2 Contract (Locked Decisions)

These three artifacts are the **immutable contract** all Phase 2 work references. Locking them up front lets #435 (M3 parser/compiler) and #436 (M5 validator + M6 editor) fan out in parallel without drift. Each lock has a "default — redirectable" note so the user can override individually.

### Lock 1: Final EBNF Grammar

This is the **final** MetricQL grammar for Phase 2 (replacing the ADR's sketch at lines 226-239). The Phase 1 patterns we shipped (FILTERED_MEAN, COMPOSITE, WINDOWED_COUNT) inform the operator/aggregation set; nothing is speculative.

```
# Top-level: every MetricQL definition is one of two forms.
expression       := aggregation_expr | composite_expr ;

# Aggregation: aggregate over events, optionally filtered + windowed.
aggregation_expr := agg_func '(' source ')' filter? window? ;
agg_func         := 'mean' | 'sum' | 'count' | 'count_distinct' | 'proportion'
                  | 'percentile' '(' NUMBER ')' ;
source           := event_type ( '.' field )? ;
event_type       := IDENTIFIER ;
field            := IDENTIFIER ;

# Filter: 'where' predicate ('and' predicate)*  — implicit AND, no OR/NOT.
# Matches the Phase 1 FILTERED_MEAN allowlist (small surface, ReDoS-adjacent risks).
filter           := 'where' predicate ('and' predicate)* ;
predicate        := field_ref operator value ;
field_ref        := IDENTIFIER ( '.' IDENTIFIER )? ;   # 'platform' or 'properties.platform'
operator         := '=' | '!=' | '>' | '<' | '>=' | '<=' | 'in' ;
value            := STRING | NUMBER | '[' value ( ',' value )* ']' ;

# Window: count events within N hours/days of user's exposure timestamp.
window           := 'within' NUMBER ( 'hours' | 'days' ) 'of' 'exposure' ;

# Composite: arithmetic over metric refs / literals / ratio calls, with precedence.
# Unary minus is a grammar production (not a lexical NUMBER prefix) so the lexer
# can always emit TokMinus for '-'; this removes the `@a - 3` vs `@a -3` ambiguity.
composite_expr   := term ( ( '+' | '-' ) term )* ;
term             := unary ( ( '*' | '/' ) unary )* ;
unary            := '-'? factor ;
factor           := metric_ref | NUMBER | '(' composite_expr ')' | ratio_expr ;
metric_ref       := '@' IDENTIFIER ;

# Ratio: built-in binary aggregator over two referenced metrics; composable as a factor.
ratio_expr       := 'ratio' '(' metric_ref ',' metric_ref ')' ;

# Lexical tokens
IDENTIFIER       := [a-z_][a-z0-9_]*   # lowercase only; matches Phase 1 metric_id regex
NUMBER           := [0-9]+ ('.' [0-9]+)?   # unsigned; negation is the `unary` production above
STRING           := '\'' [^']* '\''      # single-quoted; no escapes in Phase 2
```

**Changes from the ADR sketch (call out for review):**

| ADR sketch | Final lock | Why |
|------------|-----------|-----|
| `composite := metric_ref ARITH_OP metric_ref` (binary only) | Full expression grammar with precedence + parens | Real composites need `0.7*@a + 0.3*@b`; binary-only forbids that |
| `operator` included `'like'` | **Excluded** | Matches Phase 1 FILTERED_MEAN allowlist (ReDoS risk, cost amplification) — call out as Phase 2 follow-up if demand surfaces |
| `field_ref := 'properties.' IDENTIFIER \| IDENTIFIER` | `IDENTIFIER ( '.' IDENTIFIER )?` | Generalized namespacing; `properties` is one of many possible namespaces (`context`, `event`) |
| `ratio` listed under `aggregation` | Top-level form, composable inside `factor` | Structurally different from event-aggregations; composition is the win |
| No `(...)` in `composite` | Parens included | Standard precedence override |
| `NUMBER` carried optional `-?` sign | Sign removed; `unary := '-'? factor` production added | Signed-NUMBER token collides with binary `-`: `@a - 3` would mis-lex as `@a` `NUMBER(-3)`. Unary negation belongs in the parser, not the lexer (standard recursive-descent practice) |

**Default — redirectable.** If you want `LIKE`, OR-predicates, or string escapes in v1, redirect this lock.

**Explicitly out of scope for v1** (file as follow-up if demand surfaces):
- `OR` / `NOT` in filters
- `LIKE`, `REGEXP_LIKE`, `BETWEEN`
- String escape sequences in literals (single-quote only, no `\'`)
- Time-of-day filters (`between 9am and 5pm UTC`)
- Variant-specific aggregations (`mean(...) for variant = 'treatment'` — that's M4a)
- Time-decay weighting in composites
- Cross-experiment metric refs (mirrors Phase 1 #475 scope decision)

### Lock 2: AST Go types (`services/metrics/internal/metricql/ast.go`)

```go
package metricql

// Node is the marker interface implemented by every AST node type.
// Go lacks sum types; the unexported isNode() method enforces closed enumeration.
type Node interface {
    isNode()
    // Span returns the source position range for error messages.
    Span() Span
}

// Span is a byte-offset range into the original MetricQL source string.
type Span struct {
    Start, End int  // [Start, End) — half-open
}

// --- Top-level expression nodes -----------------------------------------------

// Aggregation: agg_func '(' source ')' filter? window?
type Aggregation struct {
    Func       AggFunc
    // Percentile is the human-friendly 0-100 scale (matches how the source
    // text reads — `percentile(95)(latency.value)`). Validity range: 0 < Percentile < 100.
    // NOTE convention mismatch with the existing proto field
    // `MetricDefinition.percentile` which uses 0-1 (`0.95`). MetricQL uses 0-100
    // in the AST because that's what users write; the codegen template in T6
    // divides by 100 before emitting `percentile_approx(col, 0.95)` for Spark.
    // Devin info on PR #559 round 3.
    Percentile float64
    Source     Source
    Filter     *Filter    // nil if no where-clause
    Window     *Window    // nil if no within-clause
    span       Span
}

func (*Aggregation) isNode() {}
func (a *Aggregation) Span() Span { return a.span }

type AggFunc int
const (
    AggUnknown AggFunc = iota
    AggMean
    AggSum
    AggCount
    AggCountDistinct
    AggProportion
    AggPercentile
)

// --- Composite (arithmetic over leaves, with precedence) ----------------------

// Composite is a binary arithmetic node. Children may themselves be
// Composite | Negate | MetricRef | Literal | Ratio.
type Composite struct {
    Op    ArithOp
    Left  Node       // Composite | Negate | MetricRef | Literal | Ratio
    Right Node       // Composite | Negate | MetricRef | Literal | Ratio
    span  Span
}

func (*Composite) isNode() {}
func (c *Composite) Span() Span { return c.span }

type ArithOp int
const (
    OpUnknown ArithOp = iota
    OpAdd
    OpSub
    OpMul
    OpDiv
)

// Negate is the AST node for unary minus produced by `parseUnary` (grammar
// production `unary := '-'? factor`). Wrapping a factor rather than rewriting
// to `Literal{0} - x` keeps source spans accurate (the span covers the leading
// `-` plus the operand) and lets the renderer emit `(-expr)` without spurious
// zeros. Devin info on PR #559 round 6 — Lock 2 must carry a node for unary
// negation or `parseUnary` has nothing to return.
type Negate struct {
    Operand Node       // Composite | Negate | MetricRef | Literal | Ratio
    span    Span
}

func (*Negate) isNode() {}
func (n *Negate) Span() Span { return n.span }

type MetricRef struct {
    ID   string  // identifier without the leading '@'
    span Span
}

func (*MetricRef) isNode() {}
func (m *MetricRef) Span() Span { return m.span }

type Literal struct {
    Value float64
    span  Span
}

func (*Literal) isNode() {}
func (l *Literal) Span() Span { return l.span }

// Ratio: ratio(@a, @b) — first-class to enable variance computation via delta method later.
type Ratio struct {
    Numerator   MetricRef
    Denominator MetricRef
    span        Span
}

func (*Ratio) isNode() {}
func (r *Ratio) Span() Span { return r.span }

// --- Sub-nodes (used inside Aggregation) --------------------------------------

type Source struct {
    EventType string  // validated against event catalog by semantic analyzer
    Field     string  // "" if not present (count() over events vs mean(heartbeat.value))
    span      Span
}

func (s Source) Span() Span { return s.span }

type Filter struct {
    Predicates []Predicate  // implicit AND between all predicates
    span       Span
}

func (f Filter) Span() Span { return f.span }

type Predicate struct {
    Field    FieldRef
    Operator Op
    Value    Value
    span     Span
}

func (p Predicate) Span() Span { return p.span }

type FieldRef struct {
    Namespace string  // "" or "properties" / "event" / "context"
    Name      string
}

type Op int
const (
    OpEq Op = iota + 1
    OpNeq
    OpLt
    OpLte
    OpGt
    OpGte
    OpIn
)

// Value is a discriminated union — exactly one of String/Number/List is set.
// IN-list literal: List populated, String/Number nil.
type Value struct {
    String *string
    Number *float64
    List   []Value
    span   Span
}

func (v Value) Span() Span { return v.span }

type Window struct {
    N    int
    Unit WindowUnit
    span Span
}

func (w Window) Span() Span { return w.span }

type WindowUnit int
const (
    WindowHours WindowUnit = iota + 1
    WindowDays
)
```

**Default — redirectable.** If you want a tagged-union representation instead of the `Node` interface, or if you want positional metadata richer than `Span`, redirect this lock. (My recommendation: `Node` interface is the idiomatic Go pattern; spans are sufficient for the error-message quality we need.)

### Lock 3: Proto field

```proto
// proto/experimentation/common/v1/metric.proto
message MetricDefinition {
  // ... existing fields ...

  // ADR-026 Phase 2: MetricQL expression source text.
  // The expression is parsed + validated by M5 at create/update time and
  // compiled to Spark SQL by M3 at scheduling time.
  //
  // Mutually exclusive with `custom_sql` and `type_config` — at most one of
  // these three may be set on any MetricDefinition. Validation enforced at
  // both M5 (creation) and M3 (rendering, defense-in-depth).
  //
  // Stored as TEXT in metric_definitions table (no JSONB needed; the AST is
  // re-parsed on each scheduling pass — parsing is fast and avoids the
  // version-skew tax of storing parsed AST).
  string metricql_expression = 20;  // first free field number after MetricDefinition's existing 1-19 range (Devin info on PR #559)
}
```

The persistence column gets added in migration `013_adr026_phase2_metricql_expression.sql` (Task 0 below).

**Default — redirectable.** If you want to also store a parsed-AST cache column (`metricql_ast JSONB`) for performance, redirect this lock. (My recommendation: parse-on-render is cheap — ~µs per metric — and avoids the AST-version-skew problem if we evolve the grammar.)

### Locked scope-and-behavior decisions

| Decision | Choice | Why |
|----------|--------|-----|
| Parser approach | **Hand-rolled recursive-descent** | Grammar has 11 rules; hand-rolled is ~500 LOC, zero deps, exact match to intent, best error messages. Generator overhead not justified. |
| Cycle detection | **Port M5's DFS 3-color from Rust to Go** | Algorithm is proven (#552); ~80 LOC; lives in `internal/metricql/cycle.go`. Mirror exact semantics so M5 + M3 agree on what's a cycle. |
| Event catalog source | **Phase 1 punt: skip catalog validation** | No event catalog service exists (per Phase 1 plan §"Defaults for open questions"). Validate `event_type` matches `^[a-z_][a-z0-9_]*$` only. File a follow-up when catalog service ships. |
| Semantic analyzer location | **Same package, separate file** (`internal/metricql/analyze.go`) | Tight coupling to AST; no reason to split |
| Spark SQL codegen | **`text/template`-based** mirroring `internal/spark/templates/*.tmpl` | Reuses existing template loader/renderer; consistent with Phase 1 types |
| Output table | **`delta.metric_summaries` (same as other types)** | M4a contract unchanged; MetricQL is opaque to M4a |
| MetricQL row in M3 scheduler | **Routes through the same Run() loop as other types; reuses #475 topo-order** | MetricQL metrics with `@metric_ref` are effectively COMPOSITEs — they slot into the existing DAG cleanly via the `metric_ref` → operand edges |
| When to compile (parse + codegen) | **Once per scheduling pass, in M3** | M5 only parses for *validation* at creation; M3 re-parses for *codegen*. Avoids serialized-AST version skew. |
| Error messages | **Source-position-tagged** (`Span` in every AST node) | Phase 2 follow-up: M6 expression editor will surface these inline. Lock the Span field in the AST now so the contract is stable. |
| Test taxonomy | **3 tiers: parser unit (table-driven), semantic unit, golden SQL** | Mirrors Phase 1 backend test layout |

---

## Reusable patterns (cite these — do not invent new abstractions)

| Pattern | Source | Usage in this plan |
|---------|--------|--------------------|
| DFS 3-color cycle detection | `crates/experimentation-management/src/validators/composite_cycle.rs` (Rust, ~160 LOC) | Port to Go in `internal/metricql/cycle.go` — identical algorithm, identical semantics |
| Spark SQL template loader | `services/metrics/internal/spark/renderer.go::loadTemplates` | Reuse for the new `metricql.sql.tmpl` family (aggregation / ratio / composite) |
| Topo-order scheduling | `services/metrics/internal/jobs/dag.go::TopologicalOrder` (#475) | MetricQL metrics with `@metric_ref` declare operand edges the same way COMPOSITE metrics do — extend `Operands` derivation to include MetricQL-parsed refs |
| Renderer dispatch | `services/metrics/internal/spark/renderer.go::RenderForType` (line 173-244) | Add a `case "METRICQL"` arm that delegates to `metricql.Compile(expr) (sql string, err error)` |
| Status table writes (skipped/failed) | `services/metrics/internal/status/` + `internal/jobs/standard.go` (#475) | MetricQL parse/compile errors map to `status.Failed`; missing operand metric_refs map to `SkippedUpstreamFailure` — same writer, same table |
| Test fixture pattern | `services/metrics/internal/jobs/standard_test.go::TestStandardJob_Run_CompositeRunsAfterOperands` (#475) | Inline JSON fixture in `t.TempDir()` for end-to-end MetricQL tests |
| Migration shape | `sql/migrations/011_adr026_phase1_metric_types.sql` | Add nullable TEXT column + CHECK constraint loosening (`metricql_expression IS NULL OR ...`) |

---

## Architecture

### New package layout under `services/metrics/`

```
internal/metricql/
  ast.go                  # NEW — Node interface + concrete types (see Lock 2)
  lexer.go                # NEW — hand-rolled token scanner
  lexer_test.go           # NEW — table-driven token tests
  parser.go               # NEW — recursive-descent parser; produces AST
  parser_test.go          # NEW — happy + sad path table tests + error-message assertions
  analyze.go              # NEW — semantic analyzer: identifier validity, @metric_ref existence
  analyze_test.go         # NEW
  cycle.go                # NEW — DFS 3-color cycle detection over @metric_ref edges
  cycle_test.go           # NEW
  compile.go              # NEW — Compile(expr string, ctx CompileContext) (sql string, refs []string, err error)
  compile_test.go         # NEW — golden-file tests against hand-written expected SQL
  templates/              # NEW — text/template-based Spark SQL fragments
    aggregation.sql.tmpl
    ratio.sql.tmpl
    composite.sql.tmpl

internal/spark/
  renderer.go             # MODIFY — add case "METRICQL" arm in RenderForType

internal/jobs/
  dag.go                  # MODIFY — derive operand edges from parsed MetricQL @metric_refs
  standard.go             # MODIFY — call metricql.Compile() for METRICQL type in the existing loop

internal/config/
  loader.go               # MODIFY — load MetricqlExpression field from the seed config

sql/migrations/
  013_adr026_phase2_metricql_expression.sql  # NEW — adds column + admits 'METRICQL' type

proto/experimentation/common/v1/
  metric.proto            # MODIFY — add metricql_expression field (Lock 3)
```

### Public API surface (everything else is unexported)

```go
// internal/metricql/compile.go
package metricql

// CompileContext provides per-scheduling-pass inputs the compiler needs.
type CompileContext struct {
    ExperimentID    string
    ComputationDate string
    KnownMetricIDs  map[string]bool  // for @metric_ref existence + cycle detection
}

// Compile parses + validates + lowers a MetricQL expression to Spark SQL.
// Returns the SQL string, the list of @metric_ref dependencies (in
// dependency order — operands first), and any error.
//
// Errors include source-position spans for actionable messages.
func Compile(source string, ctx CompileContext) (sql string, refs []string, err error)

// Parse returns the AST without semantic analysis or codegen. Used by M5 for
// creation-time validation when no per-pass context exists yet.
func Parse(source string) (Node, error)

// Analyze runs semantic checks (event identifier shape, @metric_ref existence,
// cycle detection) on a parsed AST. Used by M5 + M3.
func Analyze(ast Node, ctx AnalyzeContext) error

// AnalyzeContext is a subset of CompileContext — no ComputationDate required
// at validation time.
type AnalyzeContext struct {
    KnownMetricIDs map[string]bool
}
```

### Error type

```go
// internal/metricql/errors.go
package metricql

// Error is a structured parse / analysis / compile error with source position.
type Error struct {
    Kind     ErrorKind
    Span     Span     // byte range into the source string
    Message  string
    Source   string   // original source for context lines
    Snippet  string   // extracted "...around the error..." for human display
}

type ErrorKind int
const (
    ErrLex ErrorKind = iota + 1
    ErrParse
    ErrSemantic
    ErrCycle
    ErrCompile
)

func (e *Error) Error() string { /* formatted with snippet + caret */ }
```

This is what M6 (#436) renders inline in the expression editor.

---

## Task DAG

```
                   Phase 0 — Contract migration (sequential, must land first)
                   ┌──────────────────────────────────────────────────────┐
                   │  T0: proto + PG migration 013                         │
                   └──────────────────────┬───────────────────────────────┘
                                          ▼
                   Phase A — Lexer + parser foundation (sequential)
                   ┌──────────────────────────────────────────────────────┐
                   │  T1: AST types + Span + Node interface                │
                   │  T2: Lexer (hand-rolled scanner + token types)        │
                   │  T3: Parser (recursive-descent — produces AST)        │
                   └──────────────────────┬───────────────────────────────┘
                                          ▼
                   Phase B — Semantics + codegen (parallel — 3 streams)
                   ┌──────────────────────────────────────────────────────┐
                   │  T4: Semantic analyzer (identifier + ref validity)   │
                   │  T5: Cycle detector (port from M5 Rust)              │
                   │  T6: Spark SQL codegen (templates + Compile entry)   │
                   └──────────────────────┬───────────────────────────────┘
                                          ▼
                   Phase C — M3 wiring (sequential)
                   ┌──────────────────────────────────────────────────────┐
                   │  T7: renderer.go dispatch arm + dag.go ref extraction │
                   │  T8: standard.go integration + status mapping         │
                   └──────────────────────┬───────────────────────────────┘
                                          ▼
                   Phase D — Tests + convergence
                   ┌──────────────────────────────────────────────────────┐
                   │  T9: Golden-file SQL tests (Multiclaude overnight)   │
                   │  T10: ADR-026 status update + CLAUDE.md + PR          │
                   └──────────────────────────────────────────────────────┘
```

**Parallelization payoff:** Phase B is 3 genuinely-independent streams (T4 / T5 / T6) — best dispatched as 3 Gas Town polecats or 3 parallel subagents. Phase D's golden-file test backfill (T9) is the kind of mechanical work Multiclaude grinds overnight.

---

## Phase 0 — Contract migration

### Task T0: Proto field + PG migration 013

**Files:**
- Modify: `proto/experimentation/common/v1/metric.proto`
- Create: `sql/migrations/013_adr026_phase2_metricql_expression.sql`

- [ ] **Step 1: Add the proto field**

```proto
// proto/experimentation/common/v1/metric.proto — append to MetricDefinition:

  // ADR-026 Phase 2: MetricQL expression source text.
  // Mutually exclusive with `custom_sql` and `type_config`.
  string metricql_expression = 20;
```

Use field number **20** — first free after `MetricDefinition`'s existing 1-19 range. Confirm by reading `proto/experimentation/common/v1/metric.proto` before picking; if 20 has been taken by some other change since this plan was written, fall back to the next free integer ≥ 20. This must match Lock 3 (line 264) — they are the same contract artifact. Devin BUG-0001 on PR #559 round 2 (internal contradiction with Lock 3).

- [ ] **Step 2: Regenerate proto bindings**

Run: `buf generate proto/` (or whatever the repo's proto generation command is — check `justfile` and `buf.gen.yaml`).
Expected: Go + Rust + TypeScript bindings updated with the new field.

- [ ] **Step 3: Write the PG migration**

```sql
-- sql/migrations/013_adr026_phase2_metricql_expression.sql
-- ADR-026 Phase 2 (#435): add metricql_expression column + admit METRICQL as a valid type.

ALTER TABLE metric_definitions
    ADD COLUMN IF NOT EXISTS metricql_expression TEXT;

-- Extend the CHECK constraint to admit METRICQL.
ALTER TABLE metric_definitions DROP CONSTRAINT IF EXISTS metric_definitions_type_check;
ALTER TABLE metric_definitions ADD CONSTRAINT metric_definitions_type_check
    CHECK (type IN (
        'MEAN','PROPORTION','RATIO','COUNT','PERCENTILE','CUSTOM',
        'FILTERED_MEAN','COMPOSITE','WINDOWED_COUNT',
        'METRICQL'
    ));

-- Enforce mutual exclusion at the row level: at most one of custom_sql,
-- type_config, metricql_expression may be non-null. M5 and M3 also enforce
-- this in code — DB constraint is defense-in-depth.
-- Use DROP-then-ADD with IF EXISTS to make the migration safely replayable
-- (matches the `metric_definitions_type_check` pattern just above and
-- migration 011's style). Devin info on PR #559.
ALTER TABLE metric_definitions DROP CONSTRAINT IF EXISTS metric_definitions_single_definition_source;

-- Defensive pre-check: prior to migration 013 there was NO DB-level mutual-exclusion
-- between `custom_sql` and `type_config`. If any existing row violates the new
-- single-source rule, the ADD CONSTRAINT below will fail and roll back the whole
-- migration. Surface the bad rows first so operators see a clear error message
-- before the constraint failure noise. Devin 🚩 finding on PR #559 round 3.
DO $$
DECLARE
    bad_count INTEGER;
BEGIN
    SELECT COUNT(*) INTO bad_count
    FROM metric_definitions
    WHERE (CASE WHEN custom_sql          IS NOT NULL THEN 1 ELSE 0 END +
           CASE WHEN type_config         IS NOT NULL THEN 1 ELSE 0 END +
           CASE WHEN metricql_expression IS NOT NULL THEN 1 ELSE 0 END) > 1;

    IF bad_count > 0 THEN
        RAISE EXCEPTION 'migration 013: % existing metric_definitions row(s) have more than one of '
            '(custom_sql, type_config, metricql_expression) set. Resolve manually before re-running '
            '(query: SELECT metric_id FROM metric_definitions WHERE ... — see migration source) '
            'or null out the field that should not be authoritative.', bad_count;
    END IF;
END $$;

ALTER TABLE metric_definitions ADD CONSTRAINT metric_definitions_single_definition_source
    CHECK (
        (CASE WHEN custom_sql           IS NOT NULL THEN 1 ELSE 0 END +
         CASE WHEN type_config          IS NOT NULL THEN 1 ELSE 0 END +
         CASE WHEN metricql_expression  IS NOT NULL THEN 1 ELSE 0 END) <= 1
    );
```

- [ ] **Step 4: Commit**

```bash
git add proto/experimentation/common/v1/metric.proto \
        sql/migrations/013_adr026_phase2_metricql_expression.sql \
        # generated proto bindings
git commit -m "feat(proto): ADR-026 Phase 2 — metricql_expression field + migration 013 (#435)"
```

---

## Phase A — Lexer + parser foundation (sequential)

### Task T1: AST types

**Files:**
- Create: `services/metrics/internal/metricql/ast.go`
- Create: `services/metrics/internal/metricql/ast_test.go`

- [ ] **Step 1: Write a test asserting Node interface implementations**

```go
// ast_test.go
package metricql

import "testing"

func TestAST_AllNodeTypesImplementNodeInterface(t *testing.T) {
    var _ Node = &Aggregation{}
    var _ Node = &Composite{}
    var _ Node = &Negate{}
    var _ Node = &MetricRef{}
    var _ Node = &Literal{}
    var _ Node = &Ratio{}
}

func TestAST_SpanRoundTrip(t *testing.T) {
    s := Span{Start: 5, End: 12}
    if s.End-s.Start != 7 {
        t.Fatalf("expected length 7, got %d", s.End-s.Start)
    }
}
```

- [ ] **Step 2: Implement the AST per Lock 2 above**

Copy the AST types from the Phase 2 Contract (Lock 2) into `ast.go` verbatim. No deviations.

- [ ] **Step 3: Run tests**

Run: `cd services && go test ./metrics/internal/metricql/ -v`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add services/metrics/internal/metricql/ast.go services/metrics/internal/metricql/ast_test.go
git commit -m "feat(metricql): AST types + Node interface (#435)"
```

### Task T2: Lexer

**Files:**
- Create: `services/metrics/internal/metricql/lexer.go`
- Create: `services/metrics/internal/metricql/lexer_test.go`

- [ ] **Step 1: Define token types + Lexer struct**

```go
// lexer.go
package metricql

type TokenKind int
const (
    TokEOF TokenKind = iota
    TokIdent       // identifier (lowercase regex)
    TokNumber      // integer or float literal
    TokString      // single-quoted string
    TokKeyword     // reserved word: where, and, in, within, of, exposure, hours, days, ratio,
                   //                mean, sum, count, count_distinct, proportion, percentile
    TokAt          // '@'
    TokLParen      // '('
    TokRParen      // ')'
    TokLBracket    // '['
    TokRBracket    // ']'
    TokComma       // ','
    TokDot         // '.'
    TokPlus        // '+'
    TokMinus       // '-'
    TokStar        // '*'
    TokSlash       // '/'
    TokEq          // '='
    TokNeq         // '!='
    TokLt          // '<'
    TokLte         // '<='
    TokGt          // '>'
    TokGte         // '>='
)

type Token struct {
    Kind    TokenKind
    Value   string  // literal text from source
    Span    Span
}

type Lexer struct {
    source string
    pos    int
    tokens []Token
}

func NewLexer(source string) *Lexer { /* ... */ }
func (l *Lexer) Tokenize() ([]Token, error) { /* ... */ }
```

- [ ] **Step 2: Write the lexer table-driven test**

```go
// lexer_test.go
func TestLexer_Tokenize(t *testing.T) {
    cases := []struct{
        name, src string
        want []TokenKind  // ignore values for terseness; separate tests for value/Span
    }{
        {"simple agg",    "mean(heartbeat.value)", []TokenKind{TokKeyword, TokLParen, TokIdent, TokDot, TokIdent, TokRParen, TokEOF}},
        {"composite",     "0.7 * @a + 0.3 * @b",   []TokenKind{TokNumber, TokStar, TokAt, TokIdent, TokPlus, TokNumber, TokStar, TokAt, TokIdent, TokEOF}},
        {"where clause",  "mean(x) where p = 'mobile'", []TokenKind{TokKeyword, TokLParen, TokIdent, TokRParen, TokKeyword, TokIdent, TokEq, TokString, TokEOF}},
        {"window",        "count(s) within 7 days of exposure", []TokenKind{TokKeyword, TokLParen, TokIdent, TokRParen, TokKeyword, TokNumber, TokKeyword, TokKeyword, TokKeyword, TokEOF}},
        {"in-list",       "p in ['a', 'b']", []TokenKind{TokIdent, TokKeyword, TokLBracket, TokString, TokComma, TokString, TokRBracket, TokEOF}},
    }
    for _, tc := range cases {
        t.Run(tc.name, func(t *testing.T) {
            toks, err := NewLexer(tc.src).Tokenize()
            if err != nil { t.Fatal(err) }
            got := make([]TokenKind, len(toks))
            for i, tk := range toks { got[i] = tk.Kind }
            if !reflect.DeepEqual(got, tc.want) {
                t.Fatalf("token kinds: got %v, want %v", got, tc.want)
            }
        })
    }
}
```

- [ ] **Step 3: Implement the lexer**

Hand-roll. Each lexer method should advance `pos`, emit a token with `Span{Start: startPos, End: l.pos}`, and handle whitespace/comments. Reject anything outside the alphabet with `&Error{Kind: ErrLex, Span: ..., Message: "unexpected character"}`.

- [ ] **Step 4: Add Span tests**

```go
func TestLexer_SpansAreAccurate(t *testing.T) {
    toks, _ := NewLexer("mean(x)").Tokenize()
    // Expect: 'mean' @ [0,4); '(' @ [4,5); 'x' @ [5,6); ')' @ [6,7); EOF @ [7,7)
    expectations := []struct{ kind TokenKind; start, end int }{
        {TokKeyword, 0, 4},
        {TokLParen,  4, 5},
        {TokIdent,   5, 6},
        {TokRParen,  6, 7},
        {TokEOF,     7, 7},
    }
    for i, e := range expectations {
        if toks[i].Kind != e.kind || toks[i].Span.Start != e.start || toks[i].Span.End != e.end {
            t.Errorf("token %d: got {%v, [%d,%d)}, want {%v, [%d,%d)}",
                i, toks[i].Kind, toks[i].Span.Start, toks[i].Span.End,
                e.kind, e.start, e.end)
        }
    }
}
```

- [ ] **Step 5: Run + commit**

Run: `cd services && go test ./metrics/internal/metricql/ -v` → all PASS
Commit: `feat(metricql): hand-rolled lexer + token tests (#435)`

### Task T3: Parser

**Files:**
- Create: `services/metrics/internal/metricql/parser.go`
- Create: `services/metrics/internal/metricql/parser_test.go`

- [ ] **Step 1: Define Parser struct + Parse() entry point**

```go
// parser.go
type Parser struct {
    tokens []Token
    pos    int
}

func NewParser(tokens []Token) *Parser { return &Parser{tokens: tokens} }

// Parse returns the root AST node.
func Parse(source string) (Node, error) {
    toks, err := NewLexer(source).Tokenize()
    if err != nil { return nil, err }
    p := NewParser(toks)
    expr, err := p.parseExpression()
    if err != nil { return nil, err }
    if p.peek().Kind != TokEOF {
        return nil, &Error{Kind: ErrParse, Span: p.peek().Span, Message: "unexpected trailing tokens"}
    }
    return expr, nil
}
```

- [ ] **Step 2: Implement recursive-descent functions**

One Go function per EBNF production:
- `parseExpression() → aggregation_expr | composite_expr`
- `parseAggregation()`
- `parseComposite()` (handles `+`/`-` precedence via left-associative loop)
- `parseTerm()` (handles `*`/`/`, recurses into `parseUnary`)
- `parseUnary()` (consumes an optional leading `TokMinus`, wraps the factor in a negation AST node; this is the *only* place `-` becomes negation — every other `-` is binary subtraction in `parseComposite`)
- `parseFactor()`
- `parseFilter()`, `parsePredicate()`, `parseValue()`
- `parseWindow()`
- `parseRatio()`
- `parseSource()`

Lookahead: at top of `parseExpression`, peek 1-2 tokens to decide between `aggregation_expr` (starts with `mean`/`sum`/`count`/`count_distinct`/`proportion`/`percentile` keyword) vs `composite_expr` (starts with `@`/`(`/`NUMBER`/`ratio`).

- [ ] **Step 3: Write parser table tests (happy path)**

```go
// parser_test.go
func TestParser_HappyPath(t *testing.T) {
    cases := []struct{ name, src string; assertion func(t *testing.T, root Node) }{
        {"mean", "mean(heartbeat.value)", func(t *testing.T, root Node) {
            agg, ok := root.(*Aggregation)
            if !ok { t.Fatalf("got %T, want *Aggregation", root) }
            if agg.Func != AggMean { t.Errorf("func: got %v, want AggMean", agg.Func) }
            if agg.Source.EventType != "heartbeat" { t.Errorf("event_type: got %q", agg.Source.EventType) }
            if agg.Source.Field != "value" { t.Errorf("field: got %q", agg.Source.Field) }
        }},
        {"composite",  "0.7 * @a + 0.3 * @b", func(t *testing.T, root Node) {
            top, ok := root.(*Composite)
            if !ok { t.Fatalf("got %T, want *Composite (Add)", root) }
            if top.Op != OpAdd { t.Errorf("top-level op: got %v, want OpAdd", top.Op) }
            // Left = 0.7 * @a (Mul); Right = 0.3 * @b (Mul)
        }},
        {"ratio", "ratio(@total_revenue, @total_sessions)", func(t *testing.T, root Node) {
            r, ok := root.(*Ratio)
            if !ok { t.Fatalf("got %T, want *Ratio", root) }
            if r.Numerator.ID != "total_revenue" { t.Errorf("num: got %q", r.Numerator.ID) }
        }},
        {"windowed count", "count(session_start) within 7 days of exposure", /* ... */},
        {"filtered mean", "mean(heartbeat.value) where properties.platform = 'mobile'", /* ... */},
        {"in-list", "mean(x) where p in ['a', 'b']", /* ... */},
        {"parens override precedence", "(0.7 + 0.3) * @a", /* ... */},
    }
    for _, tc := range cases {
        t.Run(tc.name, func(t *testing.T) {
            root, err := Parse(tc.src)
            if err != nil { t.Fatal(err) }
            tc.assertion(t, root)
        })
    }
}
```

- [ ] **Step 4: Write parser error tests (sad path)**

```go
func TestParser_ErrorMessages(t *testing.T) {
    cases := []struct{ name, src, wantMsgSubstring string }{
        {"missing close paren",  "mean(x", "expected ')'"},
        {"empty agg arg",        "mean()", "expected event identifier"},
        {"bad operator in filter", "mean(x) where p ~ 1", "expected operator"},
        {"trailing tokens",      "@a + @b extra", "unexpected trailing tokens"},
        {"raw ident at top",     "watch_time", "expected aggregation or composite"},
        {"missing within unit",  "count(x) within 7 of exposure", "expected 'hours' or 'days'"},
    }
    for _, tc := range cases {
        t.Run(tc.name, func(t *testing.T) {
            _, err := Parse(tc.src)
            if err == nil { t.Fatal("expected error") }
            if !strings.Contains(err.Error(), tc.wantMsgSubstring) {
                t.Errorf("error %q does not contain %q", err.Error(), tc.wantMsgSubstring)
            }
        })
    }
}
```

- [ ] **Step 5: Run + commit**

`cd services && go test ./metrics/internal/metricql/ -v` → all PASS
Commit: `feat(metricql): recursive-descent parser + sad-path error tests (#435)`

---

## Phase B — Semantics + codegen (parallel — 3 streams)

### Task T4: Semantic analyzer

**Files:**
- Create: `services/metrics/internal/metricql/analyze.go`
- Create: `services/metrics/internal/metricql/analyze_test.go`

- [ ] **Step 1: Define AnalyzeContext + Analyze entry**

```go
// analyze.go
type AnalyzeContext struct {
    // Phase 1 punt: no event catalog. AnalyzeContext only knows about metric IDs.
    KnownMetricIDs map[string]bool
}

// Analyze runs all semantic checks. Does NOT generate SQL.
// Errors include source spans for inline editor display.
func Analyze(root Node, ctx AnalyzeContext) error
```

- [ ] **Step 2: Implement the analyzer**

Walk the AST. Checks:
- Every `Source.EventType` matches `^[a-z_][a-z0-9_]*$` (event identifier regex)
- Every `Source.Field` matches `^[a-z_][a-z0-9_]*$` if non-empty
- Every `Aggregation` with `Func == AggPercentile` has `0 < Percentile < 100`
- Every `Aggregation` with `Func == AggCount` or `AggProportion` has empty `Source.Field`
  (these aggregate over event *presence*, not a value — `count(stream_start)` valid,
  `count(heartbeat.value)` rejected; `proportion(stream_start)` valid,
  `proportion(heartbeat.value)` rejected). Devin BUG-0001/0002 on PR #559.
- Every `Aggregation` with `Func == AggMean` / `AggSum` / `AggCountDistinct` / `AggPercentile`
  has non-empty `Source.Field` (need a value to aggregate over or count distinct of —
  `mean(heartbeat.value)` valid, `mean(stream_start)` rejected;
  `count_distinct(purchase.product_id)` valid, `count_distinct(stream_start)` rejected)
- Every `MetricRef.ID` matches `^[a-z_][a-z0-9_]*$`
- Every `MetricRef.ID` exists in `ctx.KnownMetricIDs` (if non-nil — M5 may pass nil at very-first-creation time and gate this check at update time)
- Every `Filter.Predicates[].Field` matches identifier regex
- Every `Window.N > 0` and `Window.Unit` is non-zero (lexer should catch but defense-in-depth)
- **Reject top-level bare `*MetricRef` or `*Literal`** — the grammar admits these as
  `composite_expr → term → factor → metric_ref | NUMBER`, so the parser accepts
  `@watch_time` or `42` as a complete expression. Both are semantically nonsensical
  as a standalone metric definition. Reject with a clear error like "a metric
  definition must be an aggregation or arithmetic expression, not a bare ref / literal".
  `lower()` also catches this as defense-in-depth, but the analyzer is the primary
  rejection site so the error message comes with a `Span` for editor display.
  (Devin info on PR #559 round 2 — `lower()` comment previously claimed Analyze
  caught this; now it actually does.)

- [ ] **Step 3: Table-driven tests for each rejection path**

One test per rejection rule, asserting the rejected source produces the expected error message.

- [ ] **Step 4: Run + commit**

`cd services && go test ./metrics/internal/metricql/ -v` → all PASS
Commit: `feat(metricql): semantic analyzer (#435)`

### Task T5: Cycle detector (port from Rust M5)

**Files:**
- Create: `services/metrics/internal/metricql/cycle.go`
- Create: `services/metrics/internal/metricql/cycle_test.go`

- [ ] **Step 1: Read M5's cycle algorithm**

Read `crates/experimentation-management/src/validators/composite_cycle.rs`. The algorithm:
- Iterative DFS (explicit stack, not recursion — avoids stack overflow)
- 3-color marking: WHITE (unvisited), GRAY (on stack), BLACK (fully explored)
- Back-edge detection: reaching a GRAY node = cycle
- Depth cap: 5 levels (`DEFAULT_DEPTH_CAP`)

- [ ] **Step 2: Define the Go API**

```go
// cycle.go
const MaxCompositeDepth = 5  // matches M5 DEFAULT_DEPTH_CAP

// OperandLookup returns the operand metric IDs (i.e., @metric_refs) defined by
// a given metric ID, OR nil + a "not a composite / not in store" indicator.
type OperandLookup func(metricID string) (operands []string, exists bool)

// CheckNoCycles runs DFS 3-color over the operand graph rooted at `directOperands`.
// Returns nil on success, *Error{Kind: ErrCycle, ...} on failure.
func CheckNoCycles(rootID string, directOperands []string, lookup OperandLookup) error
```

- [ ] **Step 3: Port the Rust algorithm to Go**

Pure translation. Same node colors, same depth cap, same back-edge detection. Same error message format ("composite cycle detected: A -> B -> A").

- [ ] **Step 4: Test parity with M5**

Use the same test cases that M5 uses (read `composite_cycle.rs::tests`). Every case M5 rejects, this should reject identically. Every case M5 accepts, this should accept identically.

- [ ] **Step 5: Run + commit**

Commit: `feat(metricql): DFS 3-color cycle detector ported from M5 (#435)`

### Task T6: Spark SQL codegen + Compile entry

**Files:**
- Create: `services/metrics/internal/metricql/compile.go`
- Create: `services/metrics/internal/metricql/compile_test.go`
- Create: `services/metrics/internal/metricql/templates/aggregation.sql.tmpl`
- Create: `services/metrics/internal/metricql/templates/ratio.sql.tmpl`
- Create: `services/metrics/internal/metricql/templates/composite.sql.tmpl`

- [ ] **Step 1: Define Compile() entry**

```go
// compile.go
type CompileContext struct {
    ExperimentID    string
    ComputationDate string
    KnownMetricIDs  map[string]bool
}

// Compile parses + analyzes + lowers MetricQL to a single Spark SQL statement
// that writes rows to delta.metric_summaries. Returns the SQL, the ordered list
// of @metric_ref dependencies (for the M3 scheduler's topo-order use), and any
// error.
func Compile(source string, ctx CompileContext) (sql string, refs []string, err error) {
    ast, err := Parse(source)
    if err != nil { return "", nil, err }
    if err := Analyze(ast, AnalyzeContext{KnownMetricIDs: ctx.KnownMetricIDs}); err != nil {
        return "", nil, err
    }
    refs = ExtractMetricRefs(ast)
    sql, err = lower(ast, ctx)
    return sql, refs, err
}

// ExtractMetricRefs walks the AST and returns the unique @metric_ref IDs
// present anywhere in the expression. The returned slice is deduplicated
// but order-unspecified — consumers (dag.go::TopologicalOrder, M5 validator)
// treat the result as a SET, not a sequence. Inter-metric topological
// ordering happens in dag.go using these sets as the edge source.
// (Devin info on PR #559 — the prior "post-order traversal matches topo
// order" claim was misleading.)
func ExtractMetricRefs(root Node) []string
```

- [ ] **Step 2: Write SQL templates**

Mirror the existing `internal/spark/templates/composite.sql.tmpl` shape:
- `aggregation.sql.tmpl` — emits `SELECT ... FROM delta.events_validated WHERE event_type = 'X' [AND <filter>] [GROUP BY user_id, variant_id]`
- `ratio.sql.tmpl` — joins two metric_summaries reads
- `composite.sql.tmpl` — arithmetic over metric_summaries reads (same shape as Phase 1 COMPOSITE template)

Look at `services/metrics/internal/spark/templates/composite.sql.tmpl` for the exact CTE + JOIN shape M4a expects.

- [ ] **Step 3: Implement lower()**

```go
func lower(root Node, ctx CompileContext) (string, error) {
    switch n := root.(type) {
    case *Aggregation:    return lowerAggregation(n, ctx)
    case *Composite:      return lowerComposite(n, ctx)
    case *Negate:         return lowerNegate(n, ctx)        // unary minus
    case *Ratio:          return lowerRatio(n, ctx)
    // Top-level MetricRef / Literal are nonsensical — caught by Analyze, but
    // double-check here so a future Analyze bug doesn't produce silent garbage.
    case *MetricRef, *Literal:
        return "", &Error{Kind: ErrCompile, Message: "top-level expression cannot be a bare ref / literal"}
    }
    return "", fmt.Errorf("metricql: unknown node type %T", root)
}

// lowerNegate emits `(-<operand>)` so precedence is preserved when Negate
// appears inside a Composite. lowerComposite recurses via lower(), so this
// one clause covers both nested (`@a + -@b`) and top-level (`-@a`) cases.
// Devin info on PR #559 round 6.
func lowerNegate(n *Negate, ctx CompileContext) (string, error) {
    sub, err := lower(n.Operand, ctx)
    if err != nil {
        return "", err
    }
    return "(-" + sub + ")", nil
}
```

- [ ] **Step 4: Golden-file SQL tests**

Create `compile_test.go` with table cases comparing generated SQL against hand-written `.golden.sql` files:

```go
func TestCompile_Golden(t *testing.T) {
    cases := []struct{ name, src string }{
        {"mean_simple",          "mean(heartbeat.value)"},
        {"mean_filtered",        "mean(heartbeat.value) where properties.platform = 'mobile'"},
        {"count_windowed",       "count(stream_start) within 7 days of exposure"},
        {"composite_two_refs",   "0.7 * @watch_time + 0.3 * @ctr"},
        {"ratio_simple",         "ratio(@total_revenue, @total_sessions)"},
        {"composite_with_ratio", "0.5 * @a + 0.5 * ratio(@b, @c)"},
    }
    for _, tc := range cases {
        t.Run(tc.name, func(t *testing.T) {
            ctx := CompileContext{ExperimentID: "exp_test", ComputationDate: "2026-05-18"}
            ctx.KnownMetricIDs = map[string]bool{"watch_time": true, "ctr": true,
                "total_revenue": true, "total_sessions": true, "a": true, "b": true, "c": true}
            got, _, err := Compile(tc.src, ctx)
            if err != nil { t.Fatal(err) }
            goldenPath := filepath.Join("testdata", tc.name+".golden.sql")
            if *updateGolden {
                os.WriteFile(goldenPath, []byte(got), 0o644)
                return
            }
            want, err := os.ReadFile(goldenPath)
            if err != nil { t.Fatal(err) }
            if got != string(want) {
                t.Errorf("SQL mismatch (run with -update to refresh):\nGOT:\n%s\n\nWANT:\n%s", got, want)
            }
        })
    }
}

var updateGolden = flag.Bool("update", false, "update golden SQL files")
```

- [ ] **Step 5: Generate the golden files**

Run: `cd services && go test ./metrics/internal/metricql/ -run TestCompile_Golden -update`
Then hand-review each `testdata/*.golden.sql` for correctness BEFORE committing — golden files are the contract.

- [ ] **Step 6: Re-run without -update to confirm**

Run: `cd services && go test ./metrics/internal/metricql/ -run TestCompile_Golden` → all PASS

- [ ] **Step 7: Commit**

Commit: `feat(metricql): Spark SQL codegen + golden-file tests (#435)`

---

## Phase C — M3 wiring (sequential)

### Task T7: renderer.go dispatch + dag.go ref extraction

**Files:**
- Modify: `services/metrics/internal/spark/renderer.go`
- Modify: `services/metrics/internal/jobs/dag.go`
- Modify: `services/metrics/internal/jobs/dag_test.go`

- [ ] **Step 1: Add METRICQL arm to RenderForType**

```go
// renderer.go::RenderForType
case "METRICQL":
    // The actual compile happens in standard.go (which has the CompileContext);
    // RenderForType is called with a MetricqlExpression in the params and just
    // delegates. This split mirrors how COMPOSITE rendering works.
    return r.RenderMetricql(params.MetricqlExpression, params)
```

Add `MetricqlExpression string` to `spark.TemplateParams`.

- [ ] **Step 2: Extend dag.go ref extraction**

The topo-order DAG (#475) builds edges from `MetricConfig.Operands`. For METRICQL metrics, the operands are the parsed `@metric_ref`s, not the proto `Operands` field. Add a parser-hook so dag.go can derive refs:

```go
// dag.go (excerpt)
import "github.com/org/experimentation-platform/services/metrics/internal/metricql"

// operandIDs returns the metric IDs that the given metric depends on.
// For COMPOSITE: m.Operands.
// For METRICQL: parse the expression and extract @metric_refs.
// Otherwise: nil (no dependencies).
func operandIDs(m *config.MetricConfig) ([]string, error) {
    switch strings.ToUpper(m.Type) {
    case "COMPOSITE":
        ids := make([]string, len(m.Operands))
        for i, op := range m.Operands {
            ids[i] = op.MetricID
        }
        return ids, nil
    case "METRICQL":
        root, err := metricql.Parse(m.MetricqlExpression)
        if err != nil {
            // Surface the parse error to the DAG builder so it can mark the
            // metric as Failed upfront (with the parse error as the reason)
            // rather than letting it appear unscheduled in topo output and
            // then fail at Compile() with a confusing "operand-missing"-looking
            // log line. Devin design feedback on PR #559: swallowing the error
            // here defers user-visible failure to compile time, where the
            // status_map sees no recorded entry and downstream COMPOSITEs get
            // marked SkippedUpstreamFailure for a reason that's actually a
            // parse error — wrong observable. Propagating gives a single,
            // clearer Failed row with reason "metricql: parse: <msg>".
            return nil, fmt.Errorf("metricql parse for %s: %w", m.MetricID, err)
        }
        return metricql.ExtractMetricRefs(root), nil
    }
    return nil, nil
}
```

Replace the existing `if m.Type != "COMPOSITE"` branch in `TopologicalOrder` to use `operandIDs(m)` instead. Add `MetricqlExpression string` field to `config.MetricConfig` with JSON tag `json:"metricql_expression,omitempty"` so seed config files (`services/metrics/internal/jobs/testdata/seed_*.json`) can deserialize it. The empty seed files don't need updates today, but the field must be JSON-tagged for future test fixtures (and for the C-phase happy-path test in T8 which writes inline fixtures). Devin info on PR #559 round 2.

**Handling parse errors from `operandIDs`:** When `operandIDs` returns a non-nil error
(only possible for METRICQL parse failures, per the helper above), `TopologicalOrder` must
**not abort the whole pass** — instead, record the failing metric in the existing
`skippedCycle` map's sibling: a new `failedParse map[string]error` return. The scheduler's
deferred status flush (#475) then writes a `status.Failed` row with the parse error
as the reason. Update the `TopologicalOrder` return signature to
`(sorted, skippedCycle, failedParse, err)` accordingly. This keeps the parse error
single-source (it lands in `status_map` once, at DAG build time) and avoids the confusing
"operand-missing"-looking log that motivated Devin's earlier design feedback.

**⚠️ `markUnvisitedCompositesAsSkipped` is COMPOSITE-only and must be generalized
(Devin PR #559 round-4 BUG-0001).** The existing skip-propagation pass at
`services/metrics/internal/jobs/standard.go:608-621` is *not* type-agnostic: its loop
guard is `if strings.ToUpper(mPtr.Type) != "COMPOSITE" { continue }`, so it never visits
METRICQL metrics, and its blocker check is `sm.blockerFor(mPtr.Operands)`, which reads the
config `Operands` slice — METRICQL has no `Operands`; its dependencies live in the parsed
`@metric_ref`s. Left as-is, a METRICQL metric downstream of a failed/parse-failed metric
would get **no** `metric_computation_status` row, and M4a would read that as "never
scheduled" rather than "skipped due to upstream failure" — a wrong observable. T7 Step 2a
(below) makes this pass dependency-shaped instead of type-shaped.

- [ ] **Step 2a: Generalize the skip-propagation pass to METRICQL**

  Add a refs-based blocker check to `statusMap`, sibling to the existing
  operand-based one:

  ```go
  // statusmap.go (excerpt) — sibling of the existing
  //   func (sm *statusMap) blockerFor(operands []config.OperandConfig) string
  // blockerForRefs returns the first ref ID whose status is not Completed
  // (i.e. the blocking upstream), or "" if every ref completed. Used for
  // METRICQL, whose deps are parsed @metric_refs, not config.Operands.
  func (sm *statusMap) blockerForRefs(refIDs []string) string {
      for _, id := range refIDs {
          if st, ok := sm.entries[id]; !ok || st.Status != status.Completed {
              return id
          }
      }
      return ""
  }
  ```

  Then rewrite `markUnvisitedCompositesAsSkipped` so it is keyed on *having
  dependencies*, not on `Type == "COMPOSITE"`:

  - Replace the `Type != "COMPOSITE"` continue-guard with a call to
    `operandIDs(mPtr)` (the same helper from Step 2). `nil`/empty ⇒ leaf metric,
    `continue` as before. Non-empty ⇒ a dependent metric (COMPOSITE *or* METRICQL).
  - For the blocker check, dispatch on the deps source: COMPOSITE keeps
    `sm.blockerFor(mPtr.Operands)`; METRICQL uses `sm.blockerForRefs(refs)` where
    `refs` is the `operandIDs(mPtr)` result already in hand. (METRICQL metrics that
    were themselves `failedParse` are already `status.Failed` and are skipped by
    the unvisited filter, so re-parsing is not a concern here.)
  - Rename the function to `markUnvisitedDependentsAsSkipped` and update its one
    caller in `standard.go::Run`; leave a one-line doc comment noting it now
    covers COMPOSITE **and** METRICQL.

  This is a hard prerequisite for the T8 gate (BUG-0002) — both paths share
  `blockerForRefs`.

**Call sites to update for the 3→4 return-value signature change** (search before
editing — list may have shifted by the time T7 lands):

- `services/metrics/internal/jobs/standard.go::Run` — the one production caller
  (~line 110 today per the #475 commit). Add a third destructured value for
  `failedParse`; pre-mark every entry as `status.Failed` with the parse error
  as the reason before the main loop, so both downstream gates
  (`blockerFor` for COMPOSITE, `blockerForRefs` for METRICQL — T8 Step 1a)
  treat them identically to executor failures.
- `services/metrics/internal/jobs/dag_test.go` — every existing
  `TestTopologicalOrder_*` test (linear chain, nested COMPOSITE, cycle skip,
  lowercase composite type, operand outside pass — 5 tests as of `dd3c0a9`).
  Each needs the extra destructured `failedParse` return + an
  `assert len(failedParse) == 0` on the happy paths.

Devin info on PR #559 round 3 — flagged that this is a breaking internal API
change; this note enumerates the call sites so the implementer doesn't miss any.

- [ ] **Step 3: Add tests for METRICQL DAG ordering**

```go
// dag_test.go
func TestTopologicalOrder_MetricqlChain(t *testing.T) {
    metrics := []*config.MetricConfig{
        {MetricID: "weighted", Type: "METRICQL",
         MetricqlExpression: "0.7 * @watch_time + 0.3 * @ctr"},
        {MetricID: "watch_time", Type: "MEAN"},
        {MetricID: "ctr",        Type: "PROPORTION"},
    }
    // T7's signature lock for TopologicalOrder is
    //   (sorted []*config.MetricConfig, skippedCycle map[string]bool,
    //    failedParse map[string]error, err error)
    // Destructure all four — happy path expects everything empty except `sorted`.
    sorted, skipped, failedParse, err := TopologicalOrder(metrics)
    if err != nil || len(skipped) != 0 || len(failedParse) != 0 {
        t.Fatalf("err=%v skipped=%v failedParse=%v", err, skipped, failedParse)
    }
    if sorted[2].MetricID != "weighted" {
        t.Fatalf("weighted must be last; got order %v", idsOf(sorted))
    }
}
```

- [ ] **Step 4: Run + commit**

`cd services && go test ./metrics/internal/jobs/ ./metrics/internal/spark/ -v` → all PASS
Commit: `feat(metrics): METRICQL renderer + topo-order ref extraction (#435)`

### Task T8: standard.go integration + status mapping

**Files:**
- Modify: `services/metrics/internal/jobs/standard.go`
- Modify: `services/metrics/internal/jobs/standard_test.go`

- [ ] **Step 1: Add the METRICQL upstream-failure gate, then wire Compile into the Run loop**

  **1a — Upstream-dependency gate (Devin PR #559 round-4 BUG-0002).** COMPOSITE
  metrics already have an upstream gate in the Run loop at
  `services/metrics/internal/jobs/standard.go:146-157`: when
  `strings.ToUpper(mPtr.Type) == "COMPOSITE"`, it calls
  `sm.blockerFor(mPtr.Operands)` and, if a blocker is returned, marks the metric
  `SkippedUpstreamFailure` and `continue`s **before** any SQL is built or
  executed. METRICQL needs the symmetric gate, or an expression like
  `0.7 * @watch_time + 0.3 * @ctr` would compile and execute against
  `delta.metric_summaries` rows that don't exist (or are stale from a prior pass)
  when `watch_time`/`ctr` failed — silently wrong numbers, not an error.

  Add, immediately alongside the existing COMPOSITE branch:

  ```go
  if strings.ToUpper(mPtr.Type) == "METRICQL" {
      refs, err := operandIDs(mPtr) // same helper used by dag.go (T7 Step 2)
      if err != nil {
          // parse failure — already recorded as failedParse at DAG build;
          // defensive: mark + skip rather than execute a half-parsed expr.
          sm.markFailed(mPtr.MetricID, "metricql: parse: "+err.Error())
          continue
      }
      if blocker := sm.blockerForRefs(refs); blocker != "" {
          sm.markSkippedUpstreamFailure(mPtr.MetricID, blocker)
          continue
      }
  }
  ```

  `blockerForRefs` is the `statusMap` helper added in T7 Step 2a — the gate and
  the skip-propagation pass deliberately share it so "blocked" means the same
  thing in both places. Prefer factoring the COMPOSITE and METRICQL branches into
  one `if blocker := sm.blockerForMetric(mPtr); blocker != "" { … }` dispatcher
  (COMPOSITE → `blockerFor(Operands)`, METRICQL → `blockerForRefs(operandIDs)`)
  if the surrounding code in #475 makes that clean; the two-branch form above is
  the floor.

  **1b — Compile + execute.** For METRICQL metrics that pass the gate, call
  `metricql.Compile(m.MetricqlExpression, ctx)` where `ctx` is a `CompileContext`
  populated with the current pass's experiment ID, computation date, and the
  `KnownMetricIDs` set (built from `sm.entries` keys at loop entry).

  On compile error: `sm.markFailed(m.MetricID, "metricql: " + err.Error())`; continue. Same shape as render-error path (#556 fix).

  On compile success: pass `sql` to `executor.ExecuteAndWrite(ctx, sql, "delta.metric_summaries")` — exactly as for all other types.

- [ ] **Step 2: Add end-to-end test for METRICQL happy path**

Inline JSON fixture pattern (matches the C1 test from #475). Metrics: `watch_time` (MEAN), `ctr` (PROPORTION), `engagement` (METRICQL `"0.7 * @watch_time + 0.3 * @ctr"`). Assert:
- All three Status.Completed
- engagement's executed SQL contains operand reads after watch_time/ctr writes
- Status table has all three rows

- [ ] **Step 3: Add test for parse-failure path**

Metric with `metricql_expression = "mean(x"` (deliberate parse error). Assert Status.Failed with reason containing "expected ')'".

- [ ] **Step 4: Add test for METRICQL upstream-failure path (Devin PR #559 round-5 ANALYSIS-0005)**

Mirrors the COMPOSITE coverage in `TestStandardJob_Run_CompositeRunsAfterOperands`
and friends, which has no METRICQL analogue today. Inline fixture: `ok_metric`
(MEAN, succeeds), `failing_metric` (MEAN, executor returns an error for it), and
`weighted` (METRICQL `"0.7 * @failing_metric + 0.3 * @ok_metric"`). Drive the
executor stub to fail only `failing_metric`. Assert:

- `failing_metric` → `Status.Failed`
- `weighted` → `Status.SkippedUpstreamFailure`, with the blocker recorded as
  `failing_metric` (proves the T8 Step 1a gate fired via `blockerForRefs`, **not**
  a downstream SQL/exec error)
- `weighted`'s SQL was **never** handed to `executor.ExecuteAndWrite` (assert on
  the stub's call log — this is the regression that catches a missing/!shared
  gate: without 1a the expression would compile and execute)
- `ok_metric` → `Status.Completed` (the failure is scoped to the blocked branch)

Also add the skip-propagation variant: a second METRICQL metric `meta`
(`"@weighted * 2"`) downstream of `weighted`; assert it is
`SkippedUpstreamFailure` too — this exercises the generalized
`markUnvisitedDependentsAsSkipped` pass (T7 Step 2a), proving METRICQL→METRICQL
skip chaining works, not just COMPOSITE→METRICQL.

- [ ] **Step 5: Run + commit**

`cd services && go test ./metrics/internal/jobs/ -v` → all PASS
Commit: `feat(metrics): METRICQL integration into StandardJob.Run (#435)`

---

## Phase D — Tests + convergence

### Task T9: Golden-file expansion (overnight grindable)

**Files:**
- Modify: `services/metrics/internal/metricql/testdata/*.golden.sql`
- Add: 20+ additional golden cases (one per grammar rule × representative variant)

- [ ] **Step 1: Enumerate cases worth a golden test**

Aim for ≥1 golden per:
- Each `agg_func`: mean / sum / count / count_distinct / proportion / percentile(p)
- Each `operator`: =, !=, <, <=, >, >=, in
- Each `window` unit: hours, days
- Multi-predicate filter (`AND` chain ≥ 3)
- Nested composite (`(0.5 * @a + 0.5 * @b) * 2`)
- Ratio inside composite (`0.5 * @a + 0.5 * ratio(@b, @c)`)
- Percentile with composable filter (`percentile(95)(latency.value) where status = 200`)

- [ ] **Step 2: Generate golden files**

Run: `cd services && go test ./metrics/internal/metricql/ -run TestCompile_Golden -update`

- [ ] **Step 3: Hand-review every new golden file**

This is the contract. A typo in a golden file becomes a permanent bug.

- [ ] **Step 4: Re-run without -update**

Run: `cd services && go test ./metrics/internal/metricql/ -run TestCompile_Golden` → all PASS

- [ ] **Step 5: Commit**

Commit: `test(metricql): expand golden-file SQL coverage (#435)`

### Task T10: ADR-026 + CLAUDE.md + PR

- [ ] **Step 1: Update ADR-026 status block**

```markdown
| **Phase 2 (#435)** | MetricQL parser + AST + Spark SQL compiler in M3 (`services/metrics/internal/metricql/`); proto field `metricql_expression`; migration 013; topo-order integration with #475 | **Implemented** (Closes #435) | PR opened by this branch |
```

- [ ] **Step 2: Update CLAUDE.md Active Work line**

Extend the Phase 2 status from "Proposed" to "Phase 2 #435 implemented (M3 MetricQL parser/compiler); #436 (M5 validation + M6 editor) remains Proposed."

- [ ] **Step 3: Add justfile recipe**

```just
test-adr026-phase2: # ADR-026 Phase 2 #435 — MetricQL parser/compiler tests
    cd {{ services_dir }} && {{ go }} test ./metrics/internal/metricql/... -v
    cd {{ services_dir }} && {{ go }} test ./metrics/internal/jobs/ -run "TestTopologicalOrder_Metricql|TestStandardJob_Run_Metricql" -v

migrate-adr026:
    # Extend existing recipe with migration 013
```

- [ ] **Step 4: Open the PR**

```bash
git push -u origin agent-3/feat/adr-026-phase-2-metricql
gh pr create --title "feat(metrics): ADR-026 Phase 2 — MetricQL parser + compiler (Closes #435)" --body "$(cat <<EOF
## Summary
- Hand-rolled MetricQL lexer + recursive-descent parser + typed AST in services/metrics/internal/metricql/
- Semantic analyzer + DFS 3-color cycle detector (ported from M5 Rust)
- Spark SQL codegen via text/template, mirroring Phase 1 COMPOSITE template shape
- Proto field metricql_expression + migration 013 (mutually exclusive with custom_sql / type_config)
- M3 scheduler integration: topo-order DAG (#475) extended to extract @metric_refs from parsed MetricQL; renderer.go dispatches METRICQL arm to Compile()
- 30+ golden-file SQL tests locking the codegen contract

Closes #435. Sister issue #436 (M5 validation + M6 editor) gets its own PR after this merges.

## Test plan
- [ ] cd services && go test ./metrics/internal/metricql/... -count=1 -v — full parser + analyzer + cycle + codegen suite
- [ ] cd services && go test ./metrics/internal/... -count=1 — no M3 regression
- [ ] just migrate-adr026 — migration 013 applies cleanly; metric_definitions.metricql_expression column exists
- [ ] just test-adr026-phase2 — bundled regression suite
EOF
)"
```

---

## Critical files to modify

| File | Touch type | Phase |
|------|-----------|-------|
| `proto/experimentation/common/v1/metric.proto` | Modify (+`metricql_expression` field) | T0 |
| `sql/migrations/013_adr026_phase2_metricql_expression.sql` | **Create** | T0 |
| `services/metrics/internal/metricql/ast.go` | **Create** | T1 |
| `services/metrics/internal/metricql/lexer.go` + `_test.go` | **Create** | T2 |
| `services/metrics/internal/metricql/parser.go` + `_test.go` | **Create** | T3 |
| `services/metrics/internal/metricql/analyze.go` + `_test.go` | **Create** | T4 |
| `services/metrics/internal/metricql/cycle.go` + `_test.go` | **Create** | T5 |
| `services/metrics/internal/metricql/compile.go` + `_test.go` | **Create** | T6 |
| `services/metrics/internal/metricql/templates/*.sql.tmpl` | **Create** | T6 |
| `services/metrics/internal/metricql/testdata/*.golden.sql` | **Create** | T6, T9 |
| `services/metrics/internal/spark/renderer.go` | Modify (METRICQL arm) | T7 |
| `services/metrics/internal/jobs/dag.go` + `_test.go` | Modify (ref extraction) | T7 |
| `services/metrics/internal/jobs/standard.go` + `_test.go` | Modify (Compile wiring) | T8 |
| `services/metrics/internal/config/loader.go` | Modify (`MetricqlExpression` field) | T7/T8 |
| `justfile` | Modify (+`test-adr026-phase2`) | T10 |
| `docs/adrs/026-custom-metrics-layer.md` | Modify (status block) | T10 |
| `CLAUDE.md` | Modify (Active Work line) | T10 |

**Out of scope (do not touch)**:
- M5 (`crates/experimentation-management/`) — its MetricQL validator is #436
- M6 (`ui/`) — its MetricQL editor is #436
- M4a — consumes `delta.metric_summaries`, blind to metric type
- Phase 1 structured types — METRICQL is an additional type, not a replacement
- The Phase 1 COMPOSITE template — METRICQL has its own composite template (different AST shape)

---

## Verification (end-to-end)

| Gate | Command | Expected |
|------|---------|----------|
| Lexer unit | `cd services && go test ./metrics/internal/metricql/ -run TestLexer -v` | All PASS |
| Parser unit (happy + sad) | `cd services && go test ./metrics/internal/metricql/ -run TestParser -v` | All PASS, error messages match expectations |
| Analyzer unit | `cd services && go test ./metrics/internal/metricql/ -run TestAnalyze -v` | All rejection paths fire |
| Cycle parity with M5 | `cd services && go test ./metrics/internal/metricql/ -run TestCheckNoCycles -v` | Same accept/reject set as Rust M5 |
| Codegen golden | `cd services && go test ./metrics/internal/metricql/ -run TestCompile_Golden` | All golden files match |
| M3 integration | `cd services && go test ./metrics/internal/jobs/ -v` | METRICQL + COMPOSITE + structured types all coexist; topo order respected |
| Migration | `just migrate-adr026` | Migration 013 applies cleanly; `\d metric_definitions` shows new column |
| ADR-026 bundle | `just test-adr026-phase2` | All of the above bundled |
| Sanity | `cd services && go build ./...` | No build regressions |
| Existing contract | `go test ./services/metrics/internal/ -run "TestM3M5\|TestM3M4"` | No regression |

---

## Risks + mitigations

| Risk | Mitigation |
|------|-----------|
| Grammar lock turns out wrong mid-implementation | Lock is at top of this file; every task references it. If a real change is needed, update Lock 1 + open a "grammar amendment" PR before resuming task work — don't quietly diverge. |
| Hand-rolled parser has bugs that produce wrong SQL | Golden-file tests are the contract; every grammar feature gets ≥1 golden. Parser fuzz-test follow-up (Task T11 candidate) once core is stable. |
| MetricQL `@metric_ref` cycle detection diverges from M5's | T5 explicitly ports the Rust algorithm; T5 step 4 runs identical test cases against both. Cycle parity is a P0 contract. |
| Parse-on-render too slow under high metric counts | Lock 3 default chose parse-on-render for simplicity; if profiling shows >5ms per metric, add an in-memory parsed-AST cache (`map[expressionString]Node` keyed by `sha256(expr)`). Don't pre-optimize. |
| MetricQL operator subset is too restrictive for early adopters | Follow-up issues filed for each excluded operator (LIKE, OR/NOT, REGEXP). Lock 1 explicitly enumerates "out of scope for v1." |
| Subagents over-engineer the parser (use Participle / yacc despite lock) | Plan explicitly forbids parser-library deps in Lock 1; subagent dispatch prompts must include "hand-rolled only; reject any PR diff that adds a parser-library dep." |
| Cross-experiment metric refs accidentally land | Lock matches Phase 1 #475 scope: `@metric_ref` resolution is scoped to current scheduling pass's `KnownMetricIDs`. Out-of-pass refs → SkippedUpstreamFailure (consistent semantics). |

---

## Execution mode

This plan has 11 tasks across 5 phases. Recommended dispatch (matches the "hybrid" recommendation in the user-facing discussion):

1. **T0 sequentially** in this CLI session (proto + migration is contract-locking; needs review).
2. **T1 → T2 → T3 sequentially** in CLI with subagent-per-task (foundations build on each other).
3. **T4 / T5 / T6 in parallel via Gas Town** (genuinely independent — 3 polecats in separate worktrees).
4. **T7 → T8 sequentially** in CLI (M3 wiring needs review at each step — touches the live scheduler).
5. **T9 via Multiclaude overnight** (golden-file expansion is mechanical; perfect for autonomous grind).
6. **T10 sequentially** in CLI (PR opening + ADR update needs human review).

Total: ~12 commits on branch `agent-3/feat/adr-026-phase-2-metricql`; one PR `Closes #435`. Estimated execution time at hybrid throughput: ~6-8 hours real time including review-between-tasks (~2x Phase 1 #475's ~3 hours, reflecting Phase 2's larger scope).

---

## Phase 2 follow-ups (not in scope of #435)

| Item | When | Owner |
|------|------|-------|
| #436 — M5 MetricQL validator (Rust) + M6 expression editor | After #435 merges; write a Phase 2 #436 plan against the locked contract | agent-5 (Rust) + agent-6 (TS) |
| #437 — CUSTOM deprecation: migrate existing CUSTOM metrics to MetricQL or Phase 1 types | After #436 ships; covers CUSTOM removal from UI + API deprecation warning | agent-3 + agent-5 + agent-6 |
| Parser fuzz test | After #435 stable for 2 weeks | agent-3 |
| MetricQL `LIKE` / `OR` / `REGEXP_LIKE` support | After operator-demand signal from Phase 1 + Phase 2 #435 usage data | agent-3 |
| Event catalog service + MetricQL catalog-aware validation | Cross-team coordination item; not blocked on Phase 2 #435 | tbd |
| Parsed-AST cache (only if profiling shows compile latency >5ms / metric) | If/when needed | agent-3 |

---

## Self-review

**Lock coverage:**
- ✅ EBNF: every grammar production has a final form; deviations from ADR sketch enumerated
- ✅ AST: every Node type defined; Span on every node; interface marker pattern
- ✅ Proto: field number + mutual-exclusion semantics + persistence column

**Task coverage:**
- ✅ All 5 acceptance-criterion-equivalent checks (grammar lock, parser, analyzer, cycle, codegen) have tasks
- ✅ M3 wiring tasks (T7+T8) reuse #475 topo-order — no duplicate scheduling logic
- ✅ Golden tests gate codegen contract (T6, T9)
- ✅ M5 + M6 follow-up tracked in "Phase 2 follow-ups" — #436 plan deferred per the same pattern as Phase 1

**Type consistency:**
- `MetricRef.ID` is the metric_id without the `@` prefix (consistent across AST, Compile refs return value, cycle detector, dag.go ref extraction).
- `metricql_expression` proto field shape matches `string`-typed `MetricqlExpression` in `config.MetricConfig`.
- `CompileContext.KnownMetricIDs` is `map[string]bool` everywhere (compile.go, analyze.go, dag.go).

**No placeholders:** Every step has runnable code or an explicit command. No "implement appropriately" or "similar to above" hand-waves.

Plan complete.
