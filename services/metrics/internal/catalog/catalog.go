// Package catalog provides a lightweight read-only view of M5's
// `metric_definitions` table for M3's MetricQL preview path.
//
// Background (Issue #597 / ADR-026 Phase 2):
//
//   - The legacy CompileMetricqlPreview RPC required a non-empty
//     experiment_id and skipped the @metric_ref existence check by passing
//     `KnownMetricIDs: nil` to the analyzer.
//   - PR #595 made the M5 `ValidateMetricql` lint path global-scoped (no
//     experiment required); Issue #597 extends the same change to the
//     `PreviewMetricDefinition` path. Task 2 is the M3 half of that change.
//
// To reject unknown @metric_refs in the global preview path, the handler
// needs the global catalog of metric IDs. M3 and M5 share the same Postgres
// DB (see services/metrics/internal/integration_test.go:52 and
// sql/migrations/001_schema.sql:33), so M3 reads `metric_definitions`
// directly with a lean `SELECT metric_id` query.
//
// Why a separate package?
//
//   - Mirrors the established `internal/shadow/` pattern (interface + Pg
//     implementation + functional handler option).
//   - Keeps the handler package focused on RPC dispatch.
//   - Gives future tasks (experiment-scoped lookup, caching) a place to
//     live.
//
// Why `SELECT metric_id` rather than `SELECT *`? Devin's review of PR #595
// flagged that the original `list_metrics()` hot-loop fetched 18 columns
// including large JSON/SQL text blobs on every ~500ms lint cycle. Preview
// is colder (only on toggle-open + post-debounce expression change), but
// applying the perf lesson preemptively avoids the same regression here.
package catalog

import (
	"context"
	"fmt"

	"github.com/jackc/pgx/v5/pgxpool"
)

// CatalogReader is the read-only view of M5's `metric_definitions` table
// used by M3's MetricQL preview path to populate
// `metricql.AnalyzeContext.KnownMetricIDs` so unknown @metric_refs become
// SEVERITY_ERROR diagnostics in the global-scope preview.
//
// Implementations: PgPoolCatalog (production), in-test stubs (handler tests).
type CatalogReader interface {
	// ListMetricIDs returns every metric_id currently registered in M5's
	// catalog. Order is sorted by metric_id ascending so callers that care
	// about determinism (logs, golden files) don't need to re-sort.
	ListMetricIDs(ctx context.Context) ([]string, error)
}

// PgPoolCatalog is the PostgreSQL-backed implementation of CatalogReader.
// Modelled on shadow.PgStore — small, side-effect-free, no caching at this
// layer. (If preview latency becomes a problem we'll add an in-memory TTL
// cache in front; the bare query is sub-millisecond on the expected catalog
// size of <10k rows.)
type PgPoolCatalog struct {
	pool *pgxpool.Pool
}

// NewPgPoolCatalog returns a PgPoolCatalog backed by the given connection
// pool. The pool is owned by the caller (cmd/main.go); PgPoolCatalog never
// closes it.
func NewPgPoolCatalog(pool *pgxpool.Pool) *PgPoolCatalog {
	return &PgPoolCatalog{pool: pool}
}

// ListMetricIDs runs `SELECT metric_id FROM metric_definitions ORDER BY
// metric_id`. Deliberately lean: no JOINs, no JSONB columns, no SQL/MetricQL
// text blobs. (Devin perf lesson from PR #595.)
func (c *PgPoolCatalog) ListMetricIDs(ctx context.Context) ([]string, error) {
	rows, err := c.pool.Query(ctx, `SELECT metric_id FROM metric_definitions ORDER BY metric_id`)
	if err != nil {
		return nil, fmt.Errorf("catalog: list metric_ids: %w", err)
	}
	defer rows.Close()

	var ids []string
	for rows.Next() {
		var id string
		if err := rows.Scan(&id); err != nil {
			return nil, fmt.Errorf("catalog: list metric_ids scan: %w", err)
		}
		ids = append(ids, id)
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("catalog: list metric_ids iterate: %w", err)
	}
	return ids, nil
}
