# ADR-026 Phase 1: M3 Scheduler Dependency Ordering for COMPOSITE Metrics

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Issue:** [#475](https://github.com/wunderkennd/kaizen-experimentation/issues/475) — P1, `sprint-5.6`, `cluster-a`, owner `agent-3`.

**Goal:** Make M3 compute COMPOSITE metrics in topological order over their operand dependencies — so a COMPOSITE never runs before the metrics it reads from `delta.metric_summaries` have been written for the same `computation_date`.

**Architecture:** Build an in-memory dependency DAG before each scheduling pass (`metric_id` → operand `metric_id`s), topo-sort it (Kahn's algorithm), execute metrics in order, and on operand failure skip the dependent COMPOSITE + record a status row in a new `metric_computation_status` table. Defense-in-depth: detect cycles at scheduling time even though M5 already rejects them at creation (per #433 / `crates/experimentation-management/src/validators/composite_cycle.rs`). Pure scheduling change — no SQL template edits, no proto changes.

**Tech Stack:** Go 1.22 (M3 = `services/metrics/`), PostgreSQL (status table), Spark SQL via existing `executor.ExecuteAndWrite` (unchanged), Kahn's topological sort (pure Go, no new dependencies).

---

## Context

**What's already there** (do not duplicate):

- COMPOSITE template — `services/metrics/internal/spark/templates/composite.sql.tmpl` reads `delta.metric_summaries` directly. **No change needed.**
- COMPOSITE renderer dispatch — `services/metrics/internal/spark/renderer.go::RenderForType` (line 173-243) already validates operands + operator and calls `RenderComposite`. **No change needed.**
- M5 cycle detection at *creation* time — `crates/experimentation-management/src/validators/composite_cycle.rs` (iterative DFS 3-color, depth cap 5). M3 cycle detection here is *defense-in-depth at scheduling time* so the scheduler can't loop forever if a bad cycle ever slips through.
- `OperandConfig` shape — `services/metrics/internal/config/loader.go:98-101` defines `{MetricID string; Weight float64}`. Operands arrive at M3 already populated; **no M5 round-trip needed during scheduling.**
- Sequential scheduler loop — `services/metrics/internal/jobs/standard.go::Run` line 68: `for _, m := range metrics`. **This is the single insertion point** for topo-order iteration.

**What's missing** (the work):

1. No DAG / topo-sort utility anywhere in M3.
2. No way to express "metric was skipped because its operand failed" — `metric_summaries` is success-only writes; `querylog.Entry` has no status/error field and is observability-only (M4a doesn't query it).
3. No multi-metric integration test that exercises COMPOSITE-with-operand ordering.

---

## Locked-in scope decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Topo-sort algorithm | **Kahn's (BFS, in-degree based)** | Naturally detects cycles by leaving non-zero in-degree nodes unemitted. Simpler than 3-color DFS for "skip cycles, don't crash" defense-in-depth. M5 already does the deep validation. |
| Cycle handling at scheduling time | **Skip + log structured warning** | M5 has already rejected cycles at creation. If a cycle slipped through, skipping is safer than crashing the whole job. Status table records each skipped node with reason `"cycle"`. |
| Status visibility | **New `metric_computation_status` table** | `metric_summaries` schema stays untouched (avoids M4a regression). `querylog` is internal observability — M4a doesn't query it. New table is queryable by M4a and self-documenting. |
| Failure semantics for the whole job | **Collect errors, return aggregated at end** | Current fail-fast aborts before later COMPOSITEs can even attempt. Aggregated return preserves error visibility while letting the topo iteration complete. |
| Failure semantics for non-COMPOSITE | **Same: log failure, mark in status map, continue** | Was fail-fast. This is the behavior change that unblocks COMPOSITE skipping. Existing tests that rely on fail-fast (count: 0, per survey — `standard_test.go` only asserts success) need no change. |
| Failure semantics for COMPOSITE | **Skip if any operand status ≠ `completed`** | Operand could be `failed`, `skipped_upstream_failure`, or `skipped_cycle`. All three poison the COMPOSITE. |
| Cross-experiment COMPOSITEs | **Out of scope** (per issue) | Phase 1 assumes operand + COMPOSITE share `experiment_id` and `computation_date`. |
| Real-time COMPOSITE for guardrails | **Out of scope** (per issue) | Phase 1 is daily scheduling only. |
| Parallelism | **Stay sequential** | Today M3 runs metrics serially. Parallelizing the DAG is a separate optimization; doing it here would entangle correctness and concurrency review. File as follow-up if needed. |

---

## Reusable patterns (cite these — do not invent new abstractions)

| Pattern | Source | Usage in this plan |
|---------|--------|--------------------|
| Job iteration shell | `services/metrics/internal/jobs/standard.go::Run` lines 49-432 | Wrap the metric loop body in topo-order iteration; keep the existing `ExecuteAndWrite` call site unchanged |
| MetricConfig + OperandConfig | `services/metrics/internal/config/loader.go::82-101` | Read `m.Type == "COMPOSITE"` to identify nodes with edges; iterate `m.Operands` for edge targets |
| Test scaffold | `services/metrics/internal/jobs/standard_test.go::setupTestJob` (line 19) | Reuse for new dependency-ordering tests; extend `seed_adr026_phase1.json` with a 2-level COMPOSITE chain |
| Integration test harness | `services/metrics/internal/integration_test.go` (`//go:build integration`) | Mirror its docker-compose-test-stack pattern for the operand-failure-skips-dependent test |
| Migration shape | `sql/migrations/011_adr026_phase1_metric_types.sql` | Copy the `CREATE TABLE ... IF NOT EXISTS` + `CREATE INDEX` pattern; same migration tooling |
| Cycle algorithm reference | `crates/experimentation-management/src/validators/composite_cycle.rs::check_no_cycles` | Mirror only the *intent* (reject back edges, cap depth) — implementation can be Kahn's instead of DFS since our requirement is "skip cycles" not "explain the cycle path" |
| Querylog writer interface | `services/metrics/internal/querylog/pg_writer.go` | Mirror the `Writer` interface shape for the new status writer |

---

## Architecture

### New / modified files under `services/metrics/`

```
internal/jobs/
  standard.go                    # MODIFY — wrap loop in topo-order iteration, add status tracking
  dag.go                         # NEW — Kahn's topo sort + cycle detection (pure Go, no I/O)
  dag_test.go                    # NEW — unit tests for topo sort
  status_map.go                  # NEW — in-memory status tracking during a single Run
  standard_test.go               # MODIFY — add multi-metric COMPOSITE-chain tests
  integration_test.go            # MODIFY — add operand-failure-skips-dependent test
internal/status/                 # NEW package
  status.go                      # NEW — Status enum + Entry type
  pg_writer.go                   # NEW — PostgreSQL writer (mirrors querylog/pg_writer.go shape)
  mock_writer.go                 # NEW — in-memory writer for tests
sql/migrations/
  012_metric_computation_status.sql  # NEW — CREATE TABLE for status rows
```

### Status enum (the contract M4a will read against)

```go
// internal/status/status.go
package status

type Status string

const (
    StatusCompleted             Status = "completed"
    StatusFailed                Status = "failed"
    StatusSkippedUpstreamFailure Status = "skipped_upstream_failure"
    StatusSkippedCycle          Status = "skipped_cycle"
)

type Entry struct {
    ExperimentID    string
    MetricID        string
    ComputationDate string  // YYYY-MM-DD
    Status          Status
    Reason          string  // free-form explanation, e.g., "operand watch_time failed: <err>"
    RecordedAt      time.Time
}
```

### DAG types

```go
// internal/jobs/dag.go
package jobs

// TopologicalOrder returns (sorted, skipped_cycle, error).
//   - `sorted` is the topo order; iterate in this order to ensure operands run before COMPOSITEs.
//   - `skipped_cycle` is the set of metric IDs that participate in a cycle and must be skipped.
//   - `error` is non-nil only for genuine algorithmic bugs (e.g., negative in-degree) — bad input
//     produces (sorted, skipped_cycle, nil) instead.
func TopologicalOrder(metrics []*config.MetricConfig) (
    sorted []*config.MetricConfig,
    skippedCycle map[string]bool,
    err error,
)
```

### StatusMap shape

```go
// internal/jobs/status_map.go
type statusMap struct {
    entries map[string]status.Status  // metric_id → status
}

func (s *statusMap) markCompleted(metricID string)
func (s *statusMap) markFailed(metricID string, reason string) string  // returns reason for chaining
func (s *statusMap) markSkippedUpstream(metricID string, blocker string)
func (s *statusMap) markSkippedCycle(metricID string)
func (s *statusMap) blockerFor(operands []config.OperandConfig) (blocker string, ok bool)
func (s *statusMap) snapshot() map[string]status.Status  // for status writer flush
```

---

## Task DAG

```
                   Phase A — Foundations (parallel — both pure / I/O-isolated)
                   ┌──────────────────────────────────────────────────┐
                   │  A1: DAG utility (jobs/dag.go + dag_test.go)     │
                   │  A2: Status table + writer (migration + status/) │
                   └──────────────────┬───────────────────────────────┘
                                      ▼
                   Phase B — Scheduler integration (sequential, gated on A1+A2)
                   ┌──────────────────────────────────────────────────┐
                   │  B1: status_map.go + Run() topo-order rewrite    │
                   │  B2: Status writer wiring into Run() lifecycle   │
                   └──────────────────┬───────────────────────────────┘
                                      ▼
                   Phase C — Tests (parallel, gated on B2)
                   ┌──────────────────────────────────────────────────┐
                   │  C1: Multi-metric COMPOSITE-chain unit test      │
                   │  C2: Integration test — happy path multi-cycle    │
                   │  C3: Integration test — operand failure skips    │
                   └──────────────────┬───────────────────────────────┘
                                      ▼
                   Phase D — Convergence
                   ┌──────────────────────────────────────────────────┐
                   │  D1: ADR-026 status update + CLAUDE.md + PR      │
                   └──────────────────────────────────────────────────┘
```

**Parallelization payoff**: Phase A is two cleanly-separable streams (pure-Go DAG vs. SQL migration + new package). Phase C is three independent test additions — they touch different files and can be authored in parallel subagents.

---

## Phase A — Foundations (parallel)

### Task A1: DAG utility (Kahn's topo sort + cycle skip)

**Files:**
- Create: `services/metrics/internal/jobs/dag.go`
- Create: `services/metrics/internal/jobs/dag_test.go`

- [ ] **Step 1: Write the failing test for a simple two-node linear chain**

```go
// dag_test.go
package jobs

import (
    "testing"

    "github.com/wunderkennd/kaizen-experimentation/services/metrics/internal/config"
)

func TestTopologicalOrder_LinearChain(t *testing.T) {
    // operand=watch_time, composite=engagement_score depending on watch_time
    metrics := []*config.MetricConfig{
        {MetricID: "engagement_score", Type: "COMPOSITE", Operands: []config.OperandConfig{
            {MetricID: "watch_time", Weight: 1.0},
        }},
        {MetricID: "watch_time", Type: "MEAN"},
    }

    sorted, skipped, err := TopologicalOrder(metrics)
    if err != nil {
        t.Fatalf("unexpected error: %v", err)
    }
    if len(skipped) != 0 {
        t.Fatalf("expected no skipped, got %v", skipped)
    }
    if len(sorted) != 2 {
        t.Fatalf("expected 2 sorted, got %d", len(sorted))
    }
    if sorted[0].MetricID != "watch_time" {
        t.Fatalf("expected watch_time first, got %s", sorted[0].MetricID)
    }
    if sorted[1].MetricID != "engagement_score" {
        t.Fatalf("expected engagement_score second, got %s", sorted[1].MetricID)
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd services/metrics && go test ./internal/jobs/ -run TestTopologicalOrder_LinearChain -v`
Expected: `FAIL — undefined: TopologicalOrder`

- [ ] **Step 3: Implement Kahn's topo sort**

```go
// dag.go
package jobs

import (
    "github.com/wunderkennd/kaizen-experimentation/services/metrics/internal/config"
)

// TopologicalOrder returns (sorted, skipped_cycle, error). See plan §Architecture.
func TopologicalOrder(metrics []*config.MetricConfig) (
    []*config.MetricConfig,
    map[string]bool,
    error,
) {
    byID := make(map[string]*config.MetricConfig, len(metrics))
    for _, m := range metrics {
        byID[m.MetricID] = m
    }

    // Build in-degree map + adjacency: edges go FROM operand TO composite (operand must run first).
    inDeg := make(map[string]int, len(metrics))
    children := make(map[string][]string, len(metrics))
    for _, m := range metrics {
        if _, ok := inDeg[m.MetricID]; !ok {
            inDeg[m.MetricID] = 0
        }
        if m.Type != "COMPOSITE" {
            continue
        }
        for _, op := range m.Operands {
            // Only consider operands that are part of this scheduling pass; operands defined
            // elsewhere (cross-experiment, out of scope per #475) leave the COMPOSITE blocked
            // — caller skips it as `skipped_upstream_failure` with reason "operand not in pass".
            if _, ok := byID[op.MetricID]; !ok {
                continue
            }
            inDeg[m.MetricID]++
            children[op.MetricID] = append(children[op.MetricID], m.MetricID)
        }
    }

    // Kahn's: seed queue with in-degree-zero nodes, peel layers.
    queue := make([]string, 0, len(metrics))
    for id, d := range inDeg {
        if d == 0 {
            queue = append(queue, id)
        }
    }

    sorted := make([]*config.MetricConfig, 0, len(metrics))
    for len(queue) > 0 {
        id := queue[0]
        queue = queue[1:]
        sorted = append(sorted, byID[id])
        for _, child := range children[id] {
            inDeg[child]--
            if inDeg[child] == 0 {
                queue = append(queue, child)
            }
        }
    }

    // Any node still with in-degree > 0 is in (or downstream of) a cycle.
    skipped := make(map[string]bool)
    for id, d := range inDeg {
        if d > 0 {
            skipped[id] = true
        }
    }
    return sorted, skipped, nil
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd services/metrics && go test ./internal/jobs/ -run TestTopologicalOrder_LinearChain -v`
Expected: `PASS`

- [ ] **Step 5: Add test for COMPOSITE-of-COMPOSITE 3-level chain**

```go
func TestTopologicalOrder_NestedComposite(t *testing.T) {
    // a (MEAN) -> b (COMPOSITE of a) -> c (COMPOSITE of b)
    metrics := []*config.MetricConfig{
        {MetricID: "c", Type: "COMPOSITE", Operands: []config.OperandConfig{{MetricID: "b", Weight: 1}}},
        {MetricID: "b", Type: "COMPOSITE", Operands: []config.OperandConfig{{MetricID: "a", Weight: 1}}},
        {MetricID: "a", Type: "MEAN"},
    }
    sorted, skipped, _ := TopologicalOrder(metrics)
    if len(skipped) != 0 {
        t.Fatalf("expected no skipped, got %v", skipped)
    }
    got := []string{sorted[0].MetricID, sorted[1].MetricID, sorted[2].MetricID}
    want := []string{"a", "b", "c"}
    for i := range want {
        if got[i] != want[i] {
            t.Fatalf("position %d: want %s, got %s (full: %v)", i, want[i], got[i], got)
        }
    }
}
```

- [ ] **Step 6: Add test for cycle detection (defense-in-depth)**

```go
func TestTopologicalOrder_CycleIsSkipped(t *testing.T) {
    // a -> b -> a (cycle); c is independent and should still be sorted.
    metrics := []*config.MetricConfig{
        {MetricID: "a", Type: "COMPOSITE", Operands: []config.OperandConfig{{MetricID: "b", Weight: 1}}},
        {MetricID: "b", Type: "COMPOSITE", Operands: []config.OperandConfig{{MetricID: "a", Weight: 1}}},
        {MetricID: "c", Type: "MEAN"},
    }
    sorted, skipped, err := TopologicalOrder(metrics)
    if err != nil {
        t.Fatalf("expected no error (cycles are reported via skipped map), got %v", err)
    }
    if !skipped["a"] || !skipped["b"] {
        t.Fatalf("expected a + b skipped (cycle), got %v", skipped)
    }
    if len(sorted) != 1 || sorted[0].MetricID != "c" {
        t.Fatalf("expected only c sorted, got %v", sorted)
    }
}
```

- [ ] **Step 7: Add test for operand outside the pass (treated as available — caller handles)**

```go
func TestTopologicalOrder_OperandOutsidePass(t *testing.T) {
    // c references operand x that's not in this scheduling pass — c remains in-degree 0
    // (Kahn's emits it). The caller's status_map gates skipping on operand status at runtime.
    metrics := []*config.MetricConfig{
        {MetricID: "c", Type: "COMPOSITE", Operands: []config.OperandConfig{{MetricID: "x", Weight: 1}}},
    }
    sorted, skipped, _ := TopologicalOrder(metrics)
    if len(skipped) != 0 {
        t.Fatalf("expected no skipped, got %v", skipped)
    }
    if len(sorted) != 1 || sorted[0].MetricID != "c" {
        t.Fatalf("expected c sorted, got %v", sorted)
    }
}
```

- [ ] **Step 8: Run all dag_test tests**

Run: `cd services/metrics && go test ./internal/jobs/ -run TestTopologicalOrder -v`
Expected: 4 PASS

- [ ] **Step 9: Commit**

```bash
git add services/metrics/internal/jobs/dag.go services/metrics/internal/jobs/dag_test.go
git commit -m "feat(metrics): topological DAG utility for COMPOSITE scheduling (#475)"
```

---

### Task A2: Status table + writer package

**Files:**
- Create: `sql/migrations/012_metric_computation_status.sql`
- Create: `services/metrics/internal/status/status.go`
- Create: `services/metrics/internal/status/pg_writer.go`
- Create: `services/metrics/internal/status/mock_writer.go`
- Create: `services/metrics/internal/status/pg_writer_test.go`

- [ ] **Step 1: Write the migration**

```sql
-- sql/migrations/012_metric_computation_status.sql
-- ADR-026 Phase 1 follow-up (#475): record metric computation outcome per (experiment, metric, date)
-- so M4a can distinguish "missing because not scheduled" from "skipped because upstream failed".

CREATE TABLE IF NOT EXISTS metric_computation_status (
    experiment_id     TEXT NOT NULL,
    metric_id         TEXT NOT NULL,
    computation_date  DATE NOT NULL,
    status            TEXT NOT NULL CHECK (status IN (
        'completed',
        'failed',
        'skipped_upstream_failure',
        'skipped_cycle'
    )),
    reason            TEXT,
    recorded_at       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (experiment_id, metric_id, computation_date)
);

CREATE INDEX IF NOT EXISTS idx_metric_computation_status_lookup
    ON metric_computation_status (experiment_id, computation_date, status);
```

- [ ] **Step 2: Apply migration locally to verify SQL**

Run: `just migrate 012_metric_computation_status.sql`
Expected: clean apply; `psql -c "\d metric_computation_status"` shows the table.

- [ ] **Step 3: Write the status enum + Entry**

```go
// services/metrics/internal/status/status.go
package status

import "time"

type Status string

const (
    Completed              Status = "completed"
    Failed                 Status = "failed"
    SkippedUpstreamFailure Status = "skipped_upstream_failure"
    SkippedCycle           Status = "skipped_cycle"
)

type Entry struct {
    ExperimentID    string
    MetricID        string
    ComputationDate string  // YYYY-MM-DD; matches Spark SQL templates' date format
    Status          Status
    Reason          string
    RecordedAt      time.Time
}

type Writer interface {
    Write(ctx context.Context, entry Entry) error
}
```

- [ ] **Step 4: Write the PostgreSQL writer**

```go
// services/metrics/internal/status/pg_writer.go
package status

import (
    "context"
    "database/sql"
    "fmt"
)

type PgWriter struct {
    db *sql.DB
}

func NewPgWriter(db *sql.DB) *PgWriter {
    return &PgWriter{db: db}
}

func (w *PgWriter) Write(ctx context.Context, entry Entry) error {
    // UPSERT so re-runs of the same (experiment, metric, date) overwrite the prior outcome.
    _, err := w.db.ExecContext(ctx, `
        INSERT INTO metric_computation_status
            (experiment_id, metric_id, computation_date, status, reason, recorded_at)
        VALUES ($1, $2, $3, $4, $5, NOW())
        ON CONFLICT (experiment_id, metric_id, computation_date) DO UPDATE
        SET status = EXCLUDED.status,
            reason = EXCLUDED.reason,
            recorded_at = NOW()
    `, entry.ExperimentID, entry.MetricID, entry.ComputationDate, string(entry.Status), entry.Reason)
    if err != nil {
        return fmt.Errorf("status: write %s/%s/%s: %w",
            entry.ExperimentID, entry.MetricID, entry.ComputationDate, err)
    }
    return nil
}
```

- [ ] **Step 5: Write the mock writer (for unit tests)**

```go
// services/metrics/internal/status/mock_writer.go
package status

import (
    "context"
    "sync"
)

type MockWriter struct {
    mu      sync.Mutex
    Entries []Entry
}

func NewMockWriter() *MockWriter {
    return &MockWriter{Entries: nil}
}

func (w *MockWriter) Write(ctx context.Context, entry Entry) error {
    w.mu.Lock()
    defer w.mu.Unlock()
    w.Entries = append(w.Entries, entry)
    return nil
}

// Snapshot returns a copy of recorded entries (safe to inspect from tests).
func (w *MockWriter) Snapshot() []Entry {
    w.mu.Lock()
    defer w.mu.Unlock()
    out := make([]Entry, len(w.Entries))
    copy(out, w.Entries)
    return out
}
```

- [ ] **Step 6: Write the writer integration test (requires test PG)**

```go
// services/metrics/internal/status/pg_writer_test.go
//go:build integration

package status

import (
    "context"
    "database/sql"
    "os"
    "testing"
    "time"

    _ "github.com/lib/pq"
)

func TestPgWriter_UpsertOverwritesPrior(t *testing.T) {
    dsn := os.Getenv("TEST_DATABASE_URL")
    if dsn == "" {
        dsn = "postgres://experimentation:localdev@localhost:5432/experimentation?sslmode=disable"
    }
    db, err := sql.Open("postgres", dsn)
    if err != nil { t.Fatal(err) }
    defer db.Close()

    // Clean slate
    _, _ = db.Exec(`DELETE FROM metric_computation_status WHERE experiment_id = 'test_exp_475'`)

    w := NewPgWriter(db)
    ctx := context.Background()

    if err := w.Write(ctx, Entry{
        ExperimentID: "test_exp_475", MetricID: "m1", ComputationDate: "2026-05-17",
        Status: Failed, Reason: "first try", RecordedAt: time.Now(),
    }); err != nil {
        t.Fatal(err)
    }

    if err := w.Write(ctx, Entry{
        ExperimentID: "test_exp_475", MetricID: "m1", ComputationDate: "2026-05-17",
        Status: Completed, Reason: "", RecordedAt: time.Now(),
    }); err != nil {
        t.Fatal(err)
    }

    var status, reason string
    err = db.QueryRow(`
        SELECT status, COALESCE(reason, '') FROM metric_computation_status
        WHERE experiment_id = 'test_exp_475' AND metric_id = 'm1' AND computation_date = '2026-05-17'
    `).Scan(&status, &reason)
    if err != nil { t.Fatal(err) }
    if status != "completed" {
        t.Errorf("upsert failed: status = %q want completed", status)
    }
    if reason != "" {
        t.Errorf("upsert failed: reason = %q want empty", reason)
    }
}
```

- [ ] **Step 7: Run integration test**

Run: `cd services/metrics && go test -tags=integration ./internal/status/ -v`
Expected: `PASS` (requires `docker-compose -f docker-compose.test.yml up -d postgres`).

- [ ] **Step 8: Commit**

```bash
git add sql/migrations/012_metric_computation_status.sql \
        services/metrics/internal/status/status.go \
        services/metrics/internal/status/pg_writer.go \
        services/metrics/internal/status/mock_writer.go \
        services/metrics/internal/status/pg_writer_test.go
git commit -m "feat(metrics): metric_computation_status table + writer package (#475)"
```

---

## Phase B — Scheduler integration (sequential)

### Task B1: status_map + topo-order Run() rewrite

**Files:**
- Create: `services/metrics/internal/jobs/status_map.go`
- Create: `services/metrics/internal/jobs/status_map_test.go`
- Modify: `services/metrics/internal/jobs/standard.go::Run`

- [ ] **Step 1: Write the status_map unit test**

```go
// services/metrics/internal/jobs/status_map_test.go
package jobs

import (
    "testing"

    "github.com/wunderkennd/kaizen-experimentation/services/metrics/internal/config"
    "github.com/wunderkennd/kaizen-experimentation/services/metrics/internal/status"
)

func TestStatusMap_BlockerForOperands(t *testing.T) {
    sm := newStatusMap()
    sm.markCompleted("watch_time")
    sm.markFailed("ctr", "spark timeout")

    // engagement_score depends on watch_time (completed) + ctr (failed).
    operands := []config.OperandConfig{
        {MetricID: "watch_time", Weight: 1.0},
        {MetricID: "ctr", Weight: 1.0},
    }
    blocker, ok := sm.blockerFor(operands)
    if !ok {
        t.Fatalf("expected blocker, got none")
    }
    if blocker != "ctr" {
        t.Fatalf("expected blocker=ctr, got %s", blocker)
    }
}

func TestStatusMap_NoBlockerWhenAllCompleted(t *testing.T) {
    sm := newStatusMap()
    sm.markCompleted("a"); sm.markCompleted("b")
    _, ok := sm.blockerFor([]config.OperandConfig{{MetricID: "a"}, {MetricID: "b"}})
    if ok {
        t.Fatalf("expected no blocker")
    }
}

func TestStatusMap_OperandNotYetRunIsBlocker(t *testing.T) {
    // An operand not in the status map (e.g., out-of-pass or not yet processed) blocks the
    // dependent. The caller must have iterated in topo order so this case = upstream not in pass.
    sm := newStatusMap()
    sm.markCompleted("a")
    blocker, ok := sm.blockerFor([]config.OperandConfig{{MetricID: "a"}, {MetricID: "missing"}})
    if !ok || blocker != "missing" {
        t.Fatalf("expected blocker=missing, got blocker=%q ok=%v", blocker, ok)
    }
    _ = status.Completed  // keep import
}
```

- [ ] **Step 2: Implement status_map**

```go
// services/metrics/internal/jobs/status_map.go
package jobs

import (
    "github.com/wunderkennd/kaizen-experimentation/services/metrics/internal/config"
    "github.com/wunderkennd/kaizen-experimentation/services/metrics/internal/status"
)

type statusMap struct {
    entries map[string]status.Status
    reasons map[string]string
}

func newStatusMap() *statusMap {
    return &statusMap{
        entries: make(map[string]status.Status),
        reasons: make(map[string]string),
    }
}

func (s *statusMap) markCompleted(id string) {
    s.entries[id] = status.Completed
}

func (s *statusMap) markFailed(id, reason string) {
    s.entries[id] = status.Failed
    s.reasons[id] = reason
}

func (s *statusMap) markSkippedUpstream(id, blocker string) {
    s.entries[id] = status.SkippedUpstreamFailure
    s.reasons[id] = "operand " + blocker + " did not complete"
}

func (s *statusMap) markSkippedCycle(id string) {
    s.entries[id] = status.SkippedCycle
    s.reasons[id] = "metric participates in a COMPOSITE cycle"
}

// blockerFor returns the first operand whose status is not Completed (or "" if all completed).
func (s *statusMap) blockerFor(operands []config.OperandConfig) (string, bool) {
    for _, op := range operands {
        st, ok := s.entries[op.MetricID]
        if !ok || st != status.Completed {
            return op.MetricID, true
        }
    }
    return "", false
}

// statusOf returns the current status (or empty Status if unrecorded).
func (s *statusMap) statusOf(id string) status.Status {
    return s.entries[id]
}

// reasonOf returns the recorded reason (or empty string if unrecorded).
func (s *statusMap) reasonOf(id string) string {
    return s.reasons[id]
}
```

- [ ] **Step 3: Run status_map tests**

Run: `cd services/metrics && go test ./internal/jobs/ -run TestStatusMap -v`
Expected: 3 PASS

- [ ] **Step 4: Read current Run() to plan the rewrite**

Run: `sed -n '49,140p' services/metrics/internal/jobs/standard.go`
Expected: see the existing `for _, m := range metrics` loop body.

- [ ] **Step 5: Rewrite Run() to use topo order + status_map**

Replace the metric loop in `StandardJob.Run` (currently lines ~68-112 per the survey). Key changes:

```go
// services/metrics/internal/jobs/standard.go (excerpt — inside Run() after metrics are fetched)

// --- ADR-026 #475: topological scheduling for COMPOSITE metrics --------------------
sorted, cycleNodes, err := TopologicalOrder(metrics)
if err != nil {
    return fmt.Errorf("jobs: topological sort: %w", err)
}

sm := newStatusMap()
// Record cycle-skipped nodes upfront so the writer flush at the end has them.
for id := range cycleNodes {
    sm.markSkippedCycle(id)
    j.logger.Warn("metric skipped (cycle)",
        "experiment_id", experimentID,
        "metric_id", id,
        "computation_date", computationDate,
    )
}

var firstErr error
for _, m := range sorted {
    // COMPOSITE: gate on operand status BEFORE attempting execution.
    if m.Type == "COMPOSITE" {
        if blocker, blocked := sm.blockerFor(m.Operands); blocked {
            sm.markSkippedUpstream(m.MetricID, blocker)
            j.logger.Warn("composite skipped (operand failed or missing)",
                "experiment_id", experimentID,
                "metric_id", m.MetricID,
                "blocker", blocker,
                "computation_date", computationDate,
            )
            continue
        }
    }

    sql, err := j.renderer.RenderForType(m.Type, j.paramsFor(m, experiment, computationDate))
    if err != nil {
        reason := fmt.Sprintf("render: %v", err)
        sm.markFailed(m.MetricID, reason)
        if firstErr == nil {
            firstErr = fmt.Errorf("jobs: render metric %s: %w", m.MetricID, err)
        }
        j.logger.Error("metric render failed", "metric_id", m.MetricID, "err", err)
        continue
    }

    if err := j.executor.ExecuteAndWrite(ctx, sql, "delta.metric_summaries"); err != nil {
        reason := fmt.Sprintf("execute: %v", err)
        sm.markFailed(m.MetricID, reason)
        if firstErr == nil {
            firstErr = fmt.Errorf("jobs: execute metric %s: %w", m.MetricID, err)
        }
        j.logger.Error("metric execute failed", "metric_id", m.MetricID, "err", err)
        continue
    }
    sm.markCompleted(m.MetricID)
}

// Flush all status entries (including non-COMPOSITE failures and cycle-skipped) to PG.
if err := j.flushStatus(ctx, experimentID, computationDate, sm); err != nil {
    j.logger.Error("status flush failed (non-fatal)",
        "experiment_id", experimentID,
        "computation_date", computationDate,
        "err", err,
    )
}

return firstErr
```

Add the `flushStatus` helper at the bottom of `standard.go`:

```go
func (j *StandardJob) flushStatus(
    ctx context.Context,
    experimentID, computationDate string,
    sm *statusMap,
) error {
    for id, st := range sm.entries {
        entry := status.Entry{
            ExperimentID:    experimentID,
            MetricID:        id,
            ComputationDate: computationDate,
            Status:          st,
            Reason:          sm.reasonOf(id),
            RecordedAt:      time.Now(),
        }
        if err := j.statusWriter.Write(ctx, entry); err != nil {
            return err
        }
    }
    return nil
}
```

Add `statusWriter status.Writer` to the `StandardJob` struct and inject it via the constructor.

- [ ] **Step 6: Wire status writer in cmd/main.go**

```go
// services/metrics/cmd/main.go (excerpt)
statusWriter := status.NewPgWriter(db)
job := jobs.NewStandardJob(
    cfg, renderer, executor, queryLogWriter,
    jobs.WithStatusWriter(statusWriter),  // new functional option, or required ctor arg
)
```

Use whichever construction pattern matches existing `NewStandardJob`. If it currently takes positional args, add `statusWriter` as the next positional; if it uses options, add `WithStatusWriter`.

- [ ] **Step 7: Update setupTestJob() in standard_test.go to inject a MockWriter**

```go
// services/metrics/internal/jobs/standard_test.go (in setupTestJob)
mockStatus := status.NewMockWriter()
job := NewStandardJob(/* existing args */, mockStatus)
return job, mockExecutor, queryLog, mockStatus
```

- [ ] **Step 8: Run existing standard_test suite (should still pass)**

Run: `cd services/metrics && go test ./internal/jobs/ -v`
Expected: all existing tests PASS; if any rely on fail-fast on the first error, update them to assert per-metric status via `mockStatus.Snapshot()` instead.

- [ ] **Step 9: Commit**

```bash
git add services/metrics/internal/jobs/status_map.go \
        services/metrics/internal/jobs/status_map_test.go \
        services/metrics/internal/jobs/standard.go \
        services/metrics/internal/jobs/standard_test.go \
        services/metrics/cmd/main.go
git commit -m "feat(metrics): topo-order scheduling + skip-on-upstream-failure in StandardJob (#475)"
```

---

### Task B2: confirm querylog still works alongside status writer

**Files:**
- Read-only: `services/metrics/internal/querylog/*.go`

- [ ] **Step 1: Verify querylog writes still fire on success in the new loop**

Re-read the loop body in `standard.go` and confirm the existing `queryLogWriter.Write(...)` call (or equivalent) is still invoked inside the success branch (after `ExecuteAndWrite` returns nil but before `markCompleted`). If it was inside the old loop body, port it.

- [ ] **Step 2: Run integration_test in services/metrics to confirm querylog wiring**

Run: `cd services/metrics && go test -tags=integration ./internal/ -run TestComputeMetricsIntegration -v`
Expected: PASS — query_log rows still persisted as today.

- [ ] **Step 3: (If anything regressed) Commit fix; otherwise skip commit**

---

## Phase C — Tests (parallel)

### Task C1: Unit test — multi-metric COMPOSITE chain (happy path, mocked)

**Files:**
- Modify: `services/metrics/internal/jobs/standard_test.go`
- Modify: `services/metrics/internal/jobs/testdata/seed_adr026_phase1.json` (add a 2-level chain)

- [ ] **Step 1: Extend the seed file**

Add to the existing array in `seed_adr026_phase1.json`:

```json
{
  "metric_id": "session_score",
  "type": "MEAN",
  "source_event_type": "session_end",
  "value_column": "session_score"
},
{
  "metric_id": "click_rate",
  "type": "PROPORTION",
  "numerator_event_type": "click",
  "denominator_event_type": "impression"
},
{
  "metric_id": "engagement_index",
  "type": "COMPOSITE",
  "operator": "WEIGHTED_SUM",
  "operands": [
    { "metric_id": "session_score", "weight": 0.6 },
    { "metric_id": "click_rate",    "weight": 0.4 }
  ]
}
```

- [ ] **Step 2: Write the happy-path test**

```go
// standard_test.go
func TestStandardJob_Run_CompositeRunsAfterOperands(t *testing.T) {
    job, mockExec, _, mockStatus := setupTestJob(t)
    ctx := context.Background()

    if err := job.Run(ctx, "exp-475-happy"); err != nil {
        t.Fatalf("unexpected error: %v", err)
    }

    // Verify execution order: session_score + click_rate before engagement_index.
    queries := mockExec.Queries()
    var sessionIdx, clickIdx, compositeIdx = -1, -1, -1
    for i, q := range queries {
        switch {
        case strings.Contains(q, "metric_id = 'session_score'"):
            sessionIdx = i
        case strings.Contains(q, "metric_id = 'click_rate'"):
            clickIdx = i
        case strings.Contains(q, "metric_id = 'engagement_index'"):
            compositeIdx = i
        }
    }
    if sessionIdx == -1 || clickIdx == -1 || compositeIdx == -1 {
        t.Fatalf("missing metrics in execution; got order: %v", queryIDs(queries))
    }
    if compositeIdx < sessionIdx || compositeIdx < clickIdx {
        t.Fatalf("composite ran before operands: session=%d click=%d composite=%d",
            sessionIdx, clickIdx, compositeIdx)
    }

    // Verify status rows.
    snap := mockStatus.Snapshot()
    statuses := map[string]status.Status{}
    for _, e := range snap {
        statuses[e.MetricID] = e.Status
    }
    for _, id := range []string{"session_score", "click_rate", "engagement_index"} {
        if statuses[id] != status.Completed {
            t.Errorf("metric %s: status = %v want completed", id, statuses[id])
        }
    }
}
```

- [ ] **Step 3: Run the test**

Run: `cd services/metrics && go test ./internal/jobs/ -run TestStandardJob_Run_CompositeRunsAfterOperands -v`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add services/metrics/internal/jobs/standard_test.go \
        services/metrics/internal/jobs/testdata/seed_adr026_phase1.json
git commit -m "test(metrics): COMPOSITE-after-operands happy path (#475)"
```

---

### Task C2: Unit test — operand failure skips dependent COMPOSITE

**Files:**
- Modify: `services/metrics/internal/jobs/standard_test.go`

- [ ] **Step 1: Write the test using a fail-injecting executor**

```go
// standard_test.go
func TestStandardJob_Run_OperandFailureSkipsComposite(t *testing.T) {
    job, mockExec, _, mockStatus := setupTestJob(t)
    ctx := context.Background()

    // Inject a failure for any query that targets metric_id = 'click_rate'.
    mockExec.FailOn(func(sql string) bool {
        return strings.Contains(sql, "metric_id = 'click_rate'")
    })

    err := job.Run(ctx, "exp-475-skip")
    if err == nil {
        t.Fatalf("expected aggregated error from job, got nil")
    }
    if !strings.Contains(err.Error(), "click_rate") {
        t.Fatalf("expected error to mention click_rate, got: %v", err)
    }

    // engagement_index depends on click_rate → must be marked SkippedUpstreamFailure, not Failed.
    snap := mockStatus.Snapshot()
    statuses := map[string]status.Status{}
    reasons := map[string]string{}
    for _, e := range snap {
        statuses[e.MetricID] = e.Status
        reasons[e.MetricID]  = e.Reason
    }
    if statuses["click_rate"] != status.Failed {
        t.Errorf("click_rate: status = %v want failed", statuses["click_rate"])
    }
    if statuses["session_score"] != status.Completed {
        t.Errorf("session_score: status = %v want completed (independent of click_rate)",
            statuses["session_score"])
    }
    if statuses["engagement_index"] != status.SkippedUpstreamFailure {
        t.Errorf("engagement_index: status = %v want skipped_upstream_failure",
            statuses["engagement_index"])
    }
    if !strings.Contains(reasons["engagement_index"], "click_rate") {
        t.Errorf("engagement_index reason should mention click_rate, got: %q",
            reasons["engagement_index"])
    }
}
```

- [ ] **Step 2: Add FailOn() to MockExecutor if not present**

Check `services/metrics/internal/spark/executor.go` (or wherever the mock lives). If `FailOn(func(sql string) bool)` doesn't exist, add it:

```go
type MockExecutor struct {
    queries []string
    failPredicate func(string) bool
}

func (m *MockExecutor) FailOn(pred func(string) bool) {
    m.failPredicate = pred
}

func (m *MockExecutor) ExecuteAndWrite(ctx context.Context, sql, _ string) error {
    if m.failPredicate != nil && m.failPredicate(sql) {
        return fmt.Errorf("mock: injected failure")
    }
    m.queries = append(m.queries, sql)
    return nil
}
```

- [ ] **Step 3: Run the test**

Run: `cd services/metrics && go test ./internal/jobs/ -run TestStandardJob_Run_OperandFailureSkipsComposite -v`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add services/metrics/internal/jobs/standard_test.go \
        services/metrics/internal/spark/executor.go
git commit -m "test(metrics): operand failure skips dependent COMPOSITE (#475)"
```

---

### Task C3: Integration test — multi-cycle determinism with real Postgres

**Files:**
- Modify: `services/metrics/internal/integration_test.go`

- [ ] **Step 1: Write the multi-cycle test**

```go
// integration_test.go (under //go:build integration)
func TestComputeMetrics_CompositeOrdering_MultiCycle(t *testing.T) {
    db := openTestDB(t)
    defer db.Close()
    seedLayerAndExperiment(t, db, "exp-475-cycle")

    job := newRealJob(t, db)  // uses real PG status writer + MockExecutor

    // Run 3 cycles back-to-back; output must be deterministic.
    var snapshots [3][]string
    for i := 0; i < 3; i++ {
        if err := job.Run(context.Background(), "exp-475-cycle"); err != nil {
            t.Fatalf("cycle %d: %v", i, err)
        }
        rows, err := db.Query(`
            SELECT metric_id, status FROM metric_computation_status
            WHERE experiment_id = 'exp-475-cycle'
            ORDER BY metric_id
        `)
        if err != nil { t.Fatal(err) }
        var got []string
        for rows.Next() {
            var id, st string
            if err := rows.Scan(&id, &st); err != nil { t.Fatal(err) }
            got = append(got, id+"="+st)
        }
        rows.Close()
        snapshots[i] = got
    }
    if !reflect.DeepEqual(snapshots[0], snapshots[1]) || !reflect.DeepEqual(snapshots[1], snapshots[2]) {
        t.Fatalf("non-deterministic across cycles:\n  c0=%v\n  c1=%v\n  c2=%v",
            snapshots[0], snapshots[1], snapshots[2])
    }
}
```

- [ ] **Step 2: Run integration test**

Run: `just test-m3-integration` (or `cd services/metrics && go test -tags=integration ./internal/ -run TestComputeMetrics_CompositeOrdering_MultiCycle -v`)
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add services/metrics/internal/integration_test.go
git commit -m "test(metrics): multi-cycle determinism for COMPOSITE ordering (#475)"
```

---

## Phase D — Convergence

### Task D1: ADR-026 status + CLAUDE.md + PR

**Files:**
- Modify: `docs/adrs/026-custom-metrics-layer.md`
- Modify: `CLAUDE.md` (the Phase 1 status line ~94)
- Modify: `justfile` (add `test-adr026-m3` recipe)

- [ ] **Step 1: Update ADR-026 status block**

Append to the Phase 1 status section:

```markdown
**Phase 1 M3 dependency ordering (#475):** Implemented in PR #NNN (2026-MM-DD).
Topological scheduling via `services/metrics/internal/jobs/dag.go`; skip semantics
recorded in `metric_computation_status` (migration 012).
```

- [ ] **Step 2: Update CLAUDE.md Phase 1 line**

The relevant CLAUDE.md line currently reads:

> **Phase 1 implemented** (Rust M5 + M6 UI — FILTERED_MEAN, COMPOSITE, WINDOWED_COUNT; #552, #555)

Extend to:

> **Phase 1 implemented** (Rust M5 + M6 UI + M3 topo-order scheduling — FILTERED_MEAN, COMPOSITE, WINDOWED_COUNT; #552, #555, #NNN)

- [ ] **Step 3: Add justfile recipe**

```just
test-adr026-m3: # ADR-026 Phase 1 #475 — M3 dependency ordering tests
    cd services/metrics && go test ./internal/jobs/ -run "TestTopologicalOrder|TestStatusMap|TestStandardJob_Run_Composite|TestStandardJob_Run_OperandFailure" -v
    cd services/metrics && go test -tags=integration ./internal/ -run "TestComputeMetrics_CompositeOrdering" -v
```

- [ ] **Step 4: Open the PR**

```bash
git push -u origin agent-3/feat/adr-026-m3-dependency-ordering
gh pr create --title "feat(metrics): ADR-026 Phase 1 — M3 topo-order scheduling for COMPOSITE (Closes #475)" \
  --body "$(cat <<'EOF'
## Summary

- Topological scheduling of metrics in M3 via Kahn's algorithm (`services/metrics/internal/jobs/dag.go`).
- COMPOSITE metrics are gated on operand status; failed/missing operand → skip + structured log + `metric_computation_status` row.
- Defense-in-depth cycle detection at scheduling time (M5 already rejects at creation per #433).
- New `metric_computation_status` table (`sql/migrations/012`) so M4a can distinguish `missing` from `skipped_upstream_failure`.

Closes #475. Companion to ADR-026 Phase 1 backend (#552) and M6 UI (#555).

## Test plan

- [ ] `cd services/metrics && go test ./internal/jobs/ -v` — DAG + statusMap + scheduler tests pass
- [ ] `just test-adr026-m3` — bundled regression suite
- [ ] `just test-m3-integration` — multi-cycle determinism passes
- [ ] `psql -c "\d metric_computation_status"` after `just migrate` shows the new table
- [ ] Manual: kill a Spark job for one operand mid-run and confirm dependent COMPOSITE lands a `skipped_upstream_failure` row
EOF
)"
```

---

## Critical files to modify

| File | Touch type | Phase |
|------|-----------|-------|
| `services/metrics/internal/jobs/dag.go` | **Create** | A1 |
| `services/metrics/internal/jobs/dag_test.go` | **Create** | A1 |
| `sql/migrations/012_metric_computation_status.sql` | **Create** | A2 |
| `services/metrics/internal/status/status.go` | **Create** | A2 |
| `services/metrics/internal/status/pg_writer.go` | **Create** | A2 |
| `services/metrics/internal/status/mock_writer.go` | **Create** | A2 |
| `services/metrics/internal/status/pg_writer_test.go` | **Create** | A2 |
| `services/metrics/internal/jobs/status_map.go` | **Create** | B1 |
| `services/metrics/internal/jobs/status_map_test.go` | **Create** | B1 |
| `services/metrics/internal/jobs/standard.go` | Modify (rewrite loop) | B1 |
| `services/metrics/internal/jobs/standard_test.go` | Modify (+3 tests) | B1, C1, C2 |
| `services/metrics/internal/spark/executor.go` | Modify (+FailOn helper) | C2 |
| `services/metrics/cmd/main.go` | Modify (wire status writer) | B1 |
| `services/metrics/internal/integration_test.go` | Modify (+multi-cycle test) | C3 |
| `services/metrics/internal/jobs/testdata/seed_adr026_phase1.json` | Modify (+2 metrics + COMPOSITE) | C1 |
| `justfile` | Modify (+recipe) | D1 |
| `docs/adrs/026-custom-metrics-layer.md` | Modify (status block) | D1 |
| `CLAUDE.md` | Modify (Phase 1 line) | D1 |

**Out of scope (do not touch)**:
- `services/metrics/internal/spark/templates/composite.sql.tmpl` — already complete; reads `delta.metric_summaries`
- `services/metrics/internal/spark/renderer.go::RenderForType` — already complete (case "COMPOSITE")
- M4a (`crates/experimentation-analysis/`, `crates/experimentation-stats/`) — consumes the new status table later; not in this PR's scope
- M5 (`crates/experimentation-management/`) — operand validation + cycle detection at creation already shipped via #552
- Proto schemas (`proto/`) — no wire-format changes
- Parallelizing the DAG — file as follow-up if scheduler latency becomes a concern

---

## Verification (end-to-end)

| Gate | Command | Expected |
|------|---------|----------|
| DAG unit | `cd services/metrics && go test ./internal/jobs/ -run TestTopologicalOrder -v` | 4 PASS |
| statusMap unit | `cd services/metrics && go test ./internal/jobs/ -run TestStatusMap -v` | 3 PASS |
| Status writer (integration) | `cd services/metrics && go test -tags=integration ./internal/status/ -v` | PASS |
| Migration | `just migrate` | Clean apply; `\d metric_computation_status` shows the table |
| Scheduler unit (happy + skip) | `cd services/metrics && go test ./internal/jobs/ -run "TestStandardJob_Run_Composite\|TestStandardJob_Run_OperandFailure" -v` | 2 PASS |
| Full jobs suite (no regression) | `cd services/metrics && go test ./internal/jobs/ -v` | All PASS, including existing `TestStandardJob_Run_*` tests |
| Integration (multi-cycle) | `cd services/metrics && go test -tags=integration ./internal/ -run TestComputeMetrics_CompositeOrdering_MultiCycle -v` | PASS |
| ADR-026 bundle | `just test-adr026-m3` | All of the above bundled |
| Sanity build | `cd services/metrics && go build ./...` | No build regressions |
| Existing contract tests | `go test ./services/metrics/internal/ -run "TestM3M5\|TestM3M4" -v` | No regression (we don't change SQL output or wire format) |

---

## Risks + mitigations

| Risk | Mitigation |
|------|-----------|
| Behavior change from fail-fast → collect-then-return breaks an existing caller | Survey confirmed only `standard_test.go` asserts success on happy-path runs; no test relies on first-error-aborts-job semantics. If a caller does rely on early-abort, expose a `FailFast bool` option on `StandardJob`. |
| Status writer failures mask real metric failures | Status flush is best-effort: if it errors, log and continue, do not overwrite `firstErr`. Status table is observability, not authoritative. |
| Cycle detection at scheduling time loops infinitely | Kahn's terminates in O(V+E) regardless of input. Cycles are reported via `skippedCycle` map, never via infinite recursion. |
| Operand defined in a different scheduling pass (cross-experiment, out-of-scope per #475) | DAG treats out-of-pass operands as "not present" → leaves the COMPOSITE in-degree at 0 → it gets scheduled, but `statusMap.blockerFor` finds the operand un-recorded and marks it skipped at runtime. Both the DAG and the runtime check are correct in isolation. |
| `metric_computation_status` table grows unbounded | Status rows are upserted per `(experiment_id, metric_id, computation_date)` — bounded by `O(experiments × metrics × dates)`. File a retention follow-up if growth becomes a concern; not Phase 1 scope. |
| The new `metric_computation_status` table requires migration coordination with all environments | Migration is `IF NOT EXISTS` + additive only — safe to run multiple times. Add to `just migrate` recipe; no rollback needed. |
| Subagent reads this plan and over-engineers parallelism | Plan explicitly locks "stay sequential" — flag in PR description; reject any PR diff that introduces goroutines without an out-of-band conversation. |

---

## Execution mode

This plan has 9 tasks across 4 phases. Phase A is genuinely parallel (DAG vs status table); Phase C is genuinely parallel (3 independent test additions). Recommended dispatch:

1. **A1 / A2 in parallel** via subagents (single message, two `Agent` tool calls).
2. **B1 → B2 sequentially** in the CLI with subagent-per-task.
3. **C1 / C2 / C3 in parallel** via subagents.
4. **D1 sequentially** in the CLI, PR opens at the end.

Total: ~9 commits on one branch `agent-3/feat/adr-026-m3-dependency-ordering`; one PR `Closes #475` referencing the rest of the ADR-026 issue chain. Estimated execution time at subagent throughput (per #552/#555 calibration): ~2-3 hours real time including review-between-tasks.

---

## Self-review

**Spec coverage:**
- ✅ "M3 scheduler computes a metric DAG before each scheduling pass" → Task A1 + B1.
- ✅ "Scheduler executes metrics in topological order; COMPOSITE metrics run only after all their operands have completed for the same computation_date" → Task B1 (topo iteration + `blockerFor` gate).
- ✅ "Cycle detection at scheduling time (defence-in-depth)" → Task A1 (Kahn's leaves cycle nodes unemitted, reported via `skippedCycle`).
- ✅ "Failure semantics: if any operand metric fails, dependent COMPOSITEs are skipped for that computation_date and a structured warning is logged" → Task B1 (`sm.markSkippedUpstream` + `j.logger.Warn`).
- ✅ "Skipped status is reflected in metric_summaries (or an analogous status table) so M4a can distinguish 'missing' from 'failed-upstream'" → Task A2 (`metric_computation_status` table with 4-value enum).
- ✅ "Integration test: COMPOSITE metric with two operand chains runs correctly and produces deterministic output across multiple scheduling cycles" → Task C3.

**Type consistency:** `MetricConfig.Type` is string ("COMPOSITE"); `OperandConfig.MetricID` matches; `status.Status` is a string-newtype with 4 known values; `statusMap.blockerFor(operands []config.OperandConfig)` matches the operand iteration in Run().

**No placeholders:** Every step has runnable code or a concrete command. No "implement appropriately" or "similar to above."

Plan complete.
