//go:build integration

// Package metrics_integration_test validates the M2 (Event Pipeline) ↔ M3
// (Metric Computation) integration contract.
//
// These tests verify:
//   - Query log persistence to PostgreSQL (the data path M6/UI depends on)
//   - Full RPC → computation → query_log → notebook export flow
//   - SQL template alignment with M2's Delta Lake event schemas
//   - Guardrail alert schema matches what M5 consumes
//
// Requires: docker-compose.test.yml (postgres + kafka)
package metrics_test

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"regexp"
	"strings"
	"testing"

	"connectrpc.com/connect"
	"github.com/google/uuid"
	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"

	metricsv1 "github.com/org/experimentation/gen/go/experimentation/metrics/v1"
	"github.com/org/experimentation/gen/go/experimentation/metrics/v1/metricsv1connect"

	"github.com/org/experimentation-platform/services/metrics/internal/alerts"
	"github.com/org/experimentation-platform/services/metrics/internal/config"
	"github.com/org/experimentation-platform/services/metrics/internal/export"
	"github.com/org/experimentation-platform/services/metrics/internal/handler"
	"github.com/org/experimentation-platform/services/metrics/internal/jobs"
	"github.com/org/experimentation-platform/services/metrics/internal/querylog"
	"github.com/org/experimentation-platform/services/metrics/internal/spark"
	"github.com/org/experimentation-platform/services/metrics/internal/surrogate"
)

// ---------------------------------------------------------------------------
// Test infrastructure
// ---------------------------------------------------------------------------

const defaultDSN = "postgres://experimentation:localdev@localhost:5432/experimentation?sslmode=disable"

// testEnv holds all resources for an integration test.
type testEnv struct {
	pool     *pgxpool.Pool
	pgWriter *querylog.PgWriter
	client   metricsv1connect.MetricComputationServiceClient
	expID    string // experiment_id seeded in PostgreSQL (UUID)
}

// setupIntegration creates a test environment backed by the real PostgreSQL
// docker-compose.test.yml instance.  It seeds a layer + experiment so the
// query_log FK constraint is satisfied, and wires the MetricsHandler with
// a PgWriter for query log persistence.
func setupIntegration(t *testing.T) testEnv {
	t.Helper()
	ctx := context.Background()

	pool, err := pgxpool.New(ctx, defaultDSN)
	require.NoError(t, err, "cannot connect to PostgreSQL — is docker-compose.test.yml running?")
	t.Cleanup(pool.Close)

	// Verify connectivity.
	require.NoError(t, pool.Ping(ctx), "PostgreSQL ping failed")

	// Seed test data: layer → experiment (required for query_log FK).
	layerID := uuid.New()
	expID := uuid.New()
	_, err = pool.Exec(ctx, `INSERT INTO layers (layer_id, name) VALUES ($1, $2) ON CONFLICT (name) DO NOTHING`,
		layerID, fmt.Sprintf("integration-test-layer-%s", layerID))
	require.NoError(t, err)

	_, err = pool.Exec(ctx,
		`INSERT INTO experiments (experiment_id, name, owner_email, type, state, layer_id, primary_metric_id)
		 VALUES ($1, $2, $3, $4, $5, $6, $7)`,
		expID, "integration-test-exp", "test@example.com", "AB", "RUNNING", layerID, "watch_time_minutes")
	require.NoError(t, err)

	t.Cleanup(func() {
		// Clean up in reverse FK order.
		_, _ = pool.Exec(context.Background(), `DELETE FROM query_log WHERE experiment_id = $1`, expID)
		_, _ = pool.Exec(context.Background(), `DELETE FROM experiments WHERE experiment_id = $1`, expID)
		_, _ = pool.Exec(context.Background(), `DELETE FROM layers WHERE layer_id = $1`, layerID)
	})

	pgWriter := querylog.NewPgWriter(pool)

	// Build the service stack with PgWriter for query log, MockExecutor for
	// Spark SQL (no real Spark in CI).
	cfgStore, err := config.LoadFromFile("config/testdata/seed_config.json")
	require.NoError(t, err)
	renderer, err := spark.NewSQLRenderer()
	require.NoError(t, err)
	executor := spark.NewMockExecutor(500)

	stdJob := jobs.NewStandardJob(cfgStore, renderer, executor, pgWriter)
	publisher := alerts.NewMemPublisher()
	tracker := alerts.NewBreachTracker()
	vp := jobs.NewMockValueProvider()
	vp.SetVariantValue("rebuffer_rate", "f0000000-0000-0000-0000-000000000001", 0.02)
	vp.SetVariantValue("rebuffer_rate", "f0000000-0000-0000-0000-000000000002", 0.03)
	vp.SetVariantValue("error_rate", "f0000000-0000-0000-0000-000000000001", 0.005)
	vp.SetVariantValue("error_rate", "f0000000-0000-0000-0000-000000000002", 0.008)
	gj := jobs.NewGuardrailJob(cfgStore, renderer, executor, pgWriter, publisher, tracker, vp)
	ccj := jobs.NewContentConsumptionJob(cfgStore, renderer, executor, pgWriter)
	mockInputs := surrogate.InputMetrics{
		"f0000000-0000-0000-0000-000000000001": {"watch_time_minutes": 45.0, "stream_start_rate": 0.8},
		"f0000000-0000-0000-0000-000000000002": {"watch_time_minutes": 52.0, "stream_start_rate": 0.85},
	}
	surrInputProvider := &jobs.MockInputMetricsProvider{Inputs: mockInputs}
	modelLoader := surrogate.NewMockModelLoader()
	projWriter := surrogate.NewMemProjectionWriter()
	sj := jobs.NewSurrogateJob(cfgStore, renderer, surrInputProvider, pgWriter, modelLoader, projWriter)
	ilj := jobs.NewInterleavingJob(cfgStore, renderer, executor, pgWriter)

	h := handler.NewMetricsHandler(stdJob, gj, ccj, sj, ilj, nil, pgWriter)
	mux := http.NewServeMux()
	path, svcHandler := metricsv1connect.NewMetricComputationServiceHandler(h)
	mux.Handle(path, svcHandler)
	srv := httptest.NewServer(mux)
	t.Cleanup(srv.Close)

	client := metricsv1connect.NewMetricComputationServiceClient(http.DefaultClient, srv.URL)
	return testEnv{pool: pool, pgWriter: pgWriter, client: client, expID: expID.String()}
}

// ---------------------------------------------------------------------------
// Test: PostgreSQL query_log persistence
// ---------------------------------------------------------------------------

func TestPgQueryLogPersistence(t *testing.T) {
	env := setupIntegration(t)
	ctx := context.Background()

	// Write a query log entry via PgWriter.
	err := env.pgWriter.Log(ctx, querylog.Entry{
		ExperimentID: env.expID,
		MetricID:     "watch_time_minutes",
		SQLText:      "SELECT 1 AS test_query",
		RowCount:     42,
		DurationMs:   100,
		JobType:      "daily_metric",
	})
	require.NoError(t, err)

	// Read it back.
	entries, err := env.pgWriter.GetLogs(ctx, env.expID, "")
	require.NoError(t, err)
	require.Len(t, entries, 1)

	e := entries[0]
	assert.Equal(t, env.expID, e.ExperimentID)
	assert.Equal(t, "watch_time_minutes", e.MetricID)
	assert.Equal(t, "SELECT 1 AS test_query", e.SQLText)
	assert.Equal(t, int64(42), e.RowCount)
	assert.Equal(t, int64(100), e.DurationMs)
	assert.Equal(t, "daily_metric", e.JobType)
	assert.False(t, e.ComputedAt.IsZero(), "computed_at should be set by database DEFAULT NOW()")
}

func TestPgQueryLogFilterByMetric(t *testing.T) {
	env := setupIntegration(t)
	ctx := context.Background()

	// Insert entries for two different metrics.
	for _, metric := range []string{"watch_time_minutes", "stream_start_rate"} {
		err := env.pgWriter.Log(ctx, querylog.Entry{
			ExperimentID: env.expID,
			MetricID:     metric,
			SQLText:      fmt.Sprintf("SELECT * FROM delta.metric_summaries WHERE metric_id = '%s'", metric),
			RowCount:     100,
			DurationMs:   50,
			JobType:      "daily_metric",
		})
		require.NoError(t, err)
	}

	// Filter by metric_id.
	entries, err := env.pgWriter.GetLogs(ctx, env.expID, "watch_time_minutes")
	require.NoError(t, err)
	require.Len(t, entries, 1)
	assert.Equal(t, "watch_time_minutes", entries[0].MetricID)

	// Unfiltered returns both.
	all, err := env.pgWriter.GetLogs(ctx, env.expID, "")
	require.NoError(t, err)
	assert.Len(t, all, 2)
}

// ---------------------------------------------------------------------------
// Test: Full RPC → computation → query_log → notebook export
// ---------------------------------------------------------------------------

func TestComputeMetrics_PgQueryLog(t *testing.T) {
	env := setupIntegration(t)
	ctx := context.Background()

	// Use the seed experiment that exists in seed_config.json.
	seedExpID := "e0000000-0000-0000-0000-000000000001"
	seedExpUUID, err := uuid.Parse(seedExpID)
	require.NoError(t, err)

	// Seed the experiment in PostgreSQL so FK constraint is satisfied.
	layerID := uuid.New()
	_, err = env.pool.Exec(ctx,
		`INSERT INTO layers (layer_id, name) VALUES ($1, $2) ON CONFLICT (name) DO NOTHING`,
		layerID, fmt.Sprintf("seed-layer-%s", layerID))
	require.NoError(t, err)
	_, err = env.pool.Exec(ctx,
		`INSERT INTO experiments (experiment_id, name, owner_email, type, state, layer_id, primary_metric_id)
		 VALUES ($1, $2, $3, $4, $5, $6, $7) ON CONFLICT (experiment_id) DO NOTHING`,
		seedExpUUID, "homepage_recs_v2", "test@example.com", "AB", "RUNNING", layerID, "ctr_recommendation")
	require.NoError(t, err)
	t.Cleanup(func() {
		_, _ = env.pool.Exec(context.Background(), `DELETE FROM query_log WHERE experiment_id = $1`, seedExpUUID)
		_, _ = env.pool.Exec(context.Background(), `DELETE FROM experiments WHERE experiment_id = $1`, seedExpUUID)
		_, _ = env.pool.Exec(context.Background(), `DELETE FROM layers WHERE layer_id = $1`, layerID)
	})

	// Run ComputeMetrics RPC — this writes to real PostgreSQL via PgWriter.
	resp, err := env.client.ComputeMetrics(ctx, connect.NewRequest(&metricsv1.ComputeMetricsRequest{
		ExperimentId: seedExpID,
	}))
	require.NoError(t, err)
	assert.Equal(t, seedExpID, resp.Msg.GetExperimentId())
	assert.True(t, resp.Msg.GetMetricsComputed() > 0, "should compute at least 1 metric")

	// Verify query_log entries were persisted in PostgreSQL.
	var count int
	err = env.pool.QueryRow(ctx,
		`SELECT COUNT(*) FROM query_log WHERE experiment_id = $1`, seedExpUUID).Scan(&count)
	require.NoError(t, err)
	assert.True(t, count > 0, "query_log should have entries after ComputeMetrics")

	// Verify all expected job types appear in query_log.
	rows, err := env.pool.Query(ctx,
		`SELECT DISTINCT job_type FROM query_log WHERE experiment_id = $1`, seedExpUUID)
	require.NoError(t, err)
	defer rows.Close()

	jobTypes := make(map[string]bool)
	for rows.Next() {
		var jt string
		require.NoError(t, rows.Scan(&jt))
		jobTypes[jt] = true
	}
	require.NoError(t, rows.Err())

	// homepage_recs_v2 has: 4 metrics (daily_metric), rebuffer_rate is RATIO
	// (delta_method), watch_time_minutes + ctr_recommendation have CUPED
	// covariates (cuped_covariate), daily_treatment_effects, content_consumption,
	// and surrogate_input.
	assert.True(t, jobTypes["daily_metric"], "expected daily_metric job type in query_log")
	assert.True(t, jobTypes["daily_treatment_effect"], "expected daily_treatment_effect job type in query_log")
}

func TestExportNotebook_FromPgQueryLog(t *testing.T) {
	env := setupIntegration(t)
	ctx := context.Background()

	seedExpID := "e0000000-0000-0000-0000-000000000001"
	seedExpUUID, err := uuid.Parse(seedExpID)
	require.NoError(t, err)

	// Seed the experiment.
	layerID := uuid.New()
	_, _ = env.pool.Exec(ctx,
		`INSERT INTO layers (layer_id, name) VALUES ($1, $2) ON CONFLICT (name) DO NOTHING`,
		layerID, fmt.Sprintf("nb-layer-%s", layerID))
	_, _ = env.pool.Exec(ctx,
		`INSERT INTO experiments (experiment_id, name, owner_email, type, state, layer_id, primary_metric_id)
		 VALUES ($1, $2, $3, $4, $5, $6, $7) ON CONFLICT (experiment_id) DO NOTHING`,
		seedExpUUID, "homepage_recs_v2", "test@example.com", "AB", "RUNNING", layerID, "ctr_recommendation")
	t.Cleanup(func() {
		_, _ = env.pool.Exec(context.Background(), `DELETE FROM query_log WHERE experiment_id = $1`, seedExpUUID)
		_, _ = env.pool.Exec(context.Background(), `DELETE FROM experiments WHERE experiment_id = $1`, seedExpUUID)
		_, _ = env.pool.Exec(context.Background(), `DELETE FROM layers WHERE layer_id = $1`, layerID)
	})

	// First compute metrics (populates query_log).
	_, err = env.client.ComputeMetrics(ctx, connect.NewRequest(&metricsv1.ComputeMetricsRequest{
		ExperimentId: seedExpID,
	}))
	require.NoError(t, err)

	// Export notebook — backed by PgWriter reading from PostgreSQL.
	nbResp, err := env.client.ExportNotebook(ctx, connect.NewRequest(&metricsv1.ExportNotebookRequest{
		ExperimentId: seedExpID,
		NotebookFormat: "jupyter",
	}))
	require.NoError(t, err)
	assert.Contains(t, nbResp.Msg.GetFilename(), ".ipynb")

	// Validate notebook JSON structure.
	var nb export.Notebook
	err = json.Unmarshal(nbResp.Msg.GetNotebookContent(), &nb)
	require.NoError(t, err, "notebook content must be valid JSON")
	assert.Equal(t, 4, nb.NBFormat, "notebook format should be 4")
	assert.True(t, len(nb.Cells) >= 4, "notebook should have header + setup + at least 1 query pair")

	// First cell should be markdown header.
	assert.Equal(t, "markdown", nb.Cells[0].CellType)
	assert.Contains(t, strings.Join(nb.Cells[0].Source, ""), seedExpID)

	// Second cell should be code (PySpark setup).
	assert.Equal(t, "code", nb.Cells[1].CellType)
	assert.Contains(t, strings.Join(nb.Cells[1].Source, ""), "SparkSession")

	// Every code cell after the setup should contain SQL.
	for i := 3; i < len(nb.Cells); i += 2 {
		if nb.Cells[i].CellType == "code" {
			src := strings.Join(nb.Cells[i].Source, "")
			assert.Contains(t, src, "spark.sql", "code cell %d should execute SQL via spark.sql()", i)
		}
	}
}

func TestGetQueryLog_RPC(t *testing.T) {
	env := setupIntegration(t)
	ctx := context.Background()

	seedExpID := "e0000000-0000-0000-0000-000000000001"
	seedExpUUID, _ := uuid.Parse(seedExpID)
	layerID := uuid.New()
	_, _ = env.pool.Exec(ctx,
		`INSERT INTO layers (layer_id, name) VALUES ($1, $2) ON CONFLICT (name) DO NOTHING`,
		layerID, fmt.Sprintf("ql-layer-%s", layerID))
	_, _ = env.pool.Exec(ctx,
		`INSERT INTO experiments (experiment_id, name, owner_email, type, state, layer_id, primary_metric_id)
		 VALUES ($1, $2, $3, $4, $5, $6, $7) ON CONFLICT (experiment_id) DO NOTHING`,
		seedExpUUID, "homepage_recs_v2", "test@example.com", "AB", "RUNNING", layerID, "ctr_recommendation")
	t.Cleanup(func() {
		_, _ = env.pool.Exec(context.Background(), `DELETE FROM query_log WHERE experiment_id = $1`, seedExpUUID)
		_, _ = env.pool.Exec(context.Background(), `DELETE FROM experiments WHERE experiment_id = $1`, seedExpUUID)
		_, _ = env.pool.Exec(context.Background(), `DELETE FROM layers WHERE layer_id = $1`, layerID)
	})

	// Compute metrics first.
	_, err := env.client.ComputeMetrics(ctx, connect.NewRequest(&metricsv1.ComputeMetricsRequest{
		ExperimentId: seedExpID,
	}))
	require.NoError(t, err)

	// GetQueryLog — unfiltered.
	resp, err := env.client.GetQueryLog(ctx, connect.NewRequest(&metricsv1.GetQueryLogRequest{
		ExperimentId: seedExpID,
	}))
	require.NoError(t, err)
	assert.True(t, len(resp.Msg.GetEntries()) > 0, "query log should have entries")

	// Every entry should have non-empty SQL text and experiment_id.
	for _, entry := range resp.Msg.GetEntries() {
		assert.NotEmpty(t, entry.GetSqlText(), "sql_text must not be empty")
		assert.Equal(t, seedExpID, entry.GetExperimentId())
	}

	// Filter by metric — watch_time_minutes is a secondary metric with CUPED.
	filtered, err := env.client.GetQueryLog(ctx, connect.NewRequest(&metricsv1.GetQueryLogRequest{
		ExperimentId: seedExpID,
		MetricId:     "watch_time_minutes",
	}))
	require.NoError(t, err)
	assert.True(t, len(filtered.Msg.GetEntries()) > 0, "should have log entries for watch_time_minutes")
	assert.True(t, len(filtered.Msg.GetEntries()) < len(resp.Msg.GetEntries()),
		"filtered results should be fewer than unfiltered")
}

// ---------------------------------------------------------------------------
// Test: SQL template ↔ M2 event schema alignment
//
// These tests verify that rendered SQL templates reference the correct Delta
// Lake tables and columns as produced by M2's Kafka Connect sink. This is the
// core M2↔M3 contract: M2 writes to Delta Lake tables, M3 reads them via SQL.
// ---------------------------------------------------------------------------

// m2DeltaSchema defines the expected column sets for each Delta Lake table
// that M2's Kafka Connect sink populates. Any SQL template that references
// a column not in this set indicates a contract misalignment.
var m2DeltaSchema = map[string][]string{
	"delta.exposures": {
		"event_id", "experiment_id", "user_id", "variant_id", "platform",
		"session_id", "assignment_probability", "interleaving_provenance",
		"bandit_context_json", "lifecycle_segment", "event_timestamp",
		"ingested_at", "date_partition",
	},
	"delta.metric_events": {
		"event_id", "user_id", "event_type", "value", "content_id",
		"session_id", "properties", "event_timestamp", "ingested_at",
		"date_partition",
	},
	"delta.qoe_events": {
		"event_id", "session_id", "content_id", "user_id",
		"time_to_first_frame_ms", "rebuffer_count", "rebuffer_ratio",
		"avg_bitrate_kbps", "resolution_switches", "peak_resolution_height",
		"startup_failure_rate", "playback_duration_ms",
		"cdn_provider", "abr_algorithm", "encoding_profile",
		"event_timestamp", "ingested_at", "date_partition",
	},
	"delta.metric_summaries": {
		"experiment_id", "user_id", "variant_id", "metric_id",
		"lifecycle_segment", "metric_value", "cuped_covariate",
		"session_count", "computation_date",
	},
	"delta.reward_events": {
		"event_id", "experiment_id", "user_id", "arm_id", "reward",
		"context_json", "event_timestamp", "ingested_at", "date_partition",
	},
}

// templateTestCase represents a single SQL template rendering scenario.
type templateTestCase struct {
	name   string
	params spark.TemplateParams
	render func(r *spark.SQLRenderer, p spark.TemplateParams) (string, error)
	// expectedTables lists Delta Lake tables the SQL should reference.
	expectedTables []string
	// expectedColumns lists column names that must appear in the SQL.
	expectedColumns []string
}

func TestSQLTemplateEventSchemaAlignment(t *testing.T) {
	renderer, err := spark.NewSQLRenderer()
	require.NoError(t, err)

	expID := "e0000000-0000-0000-0000-000000000001"
	compDate := "2024-01-15"

	cases := []templateTestCase{
		{
			name: "mean metric references exposures + metric_events",
			params: spark.TemplateParams{
				ExperimentID: expID, MetricID: "watch_time_minutes",
				SourceEventType: "heartbeat", ComputationDate: compDate,
			},
			render:          func(r *spark.SQLRenderer, p spark.TemplateParams) (string, error) { return r.RenderMean(p) },
			expectedTables:  []string{"delta.exposures", "delta.metric_events"},
			expectedColumns: []string{"user_id", "variant_id", "experiment_id", "event_type", "value"},
		},
		{
			name: "proportion metric references exposures + metric_events",
			params: spark.TemplateParams{
				ExperimentID: expID, MetricID: "stream_start_rate",
				SourceEventType: "stream_start", ComputationDate: compDate,
			},
			render:          func(r *spark.SQLRenderer, p spark.TemplateParams) (string, error) { return r.RenderProportion(p) },
			expectedTables:  []string{"delta.exposures", "delta.metric_events"},
			expectedColumns: []string{"user_id", "variant_id", "experiment_id", "event_type", "value"},
		},
		{
			name: "ratio metric references exposures + metric_events for numerator/denominator",
			params: spark.TemplateParams{
				ExperimentID: expID, MetricID: "rebuffer_rate",
				NumeratorEventType: "rebuffer_event", DenominatorEventType: "playback_minute",
				ComputationDate: compDate,
			},
			render:          func(r *spark.SQLRenderer, p spark.TemplateParams) (string, error) { return r.RenderRatio(p) },
			expectedTables:  []string{"delta.exposures", "delta.metric_events"},
			expectedColumns: []string{"user_id", "variant_id", "experiment_id", "event_type", "value"},
		},
		{
			name: "ratio delta method references exposures + metric_events",
			params: spark.TemplateParams{
				ExperimentID: expID, MetricID: "rebuffer_rate",
				NumeratorEventType: "rebuffer_event", DenominatorEventType: "playback_minute",
				ComputationDate: compDate,
			},
			render:          func(r *spark.SQLRenderer, p spark.TemplateParams) (string, error) { return r.RenderRatioDeltaMethod(p) },
			expectedTables:  []string{"delta.exposures", "delta.metric_events"},
			expectedColumns: []string{"user_id", "variant_id", "experiment_id", "event_type", "value"},
		},
		{
			name: "QoE metric references exposures + qoe_events",
			params: spark.TemplateParams{
				ExperimentID: expID, MetricID: "ttff_mean",
				QoEField: "time_to_first_frame_ms", ComputationDate: compDate,
			},
			render:          func(r *spark.SQLRenderer, p spark.TemplateParams) (string, error) { return r.RenderQoEMetric(p) },
			expectedTables:  []string{"delta.exposures", "delta.qoe_events"},
			expectedColumns: []string{"user_id", "variant_id", "experiment_id", "time_to_first_frame_ms"},
		},
		{
			name: "CUPED covariate references exposures + metric_events with date filter",
			params: spark.TemplateParams{
				ExperimentID: expID, MetricID: "watch_time_minutes",
				CupedEnabled: true, CupedCovariateEventType: "heartbeat",
				ExperimentStartDate: "2024-01-08", CupedLookbackDays: 7,
				ComputationDate: compDate,
			},
			render:          func(r *spark.SQLRenderer, p spark.TemplateParams) (string, error) { return r.RenderCupedCovariate(p) },
			expectedTables:  []string{"delta.exposures", "delta.metric_events"},
			expectedColumns: []string{"user_id", "variant_id", "experiment_id", "event_type", "value"},
		},
		{
			name: "guardrail metric references exposures + metric_events",
			params: spark.TemplateParams{
				ExperimentID: expID, MetricID: "rebuffer_rate",
				SourceEventType: "qoe_rebuffer", ComputationDate: compDate,
			},
			render:          func(r *spark.SQLRenderer, p spark.TemplateParams) (string, error) { return r.RenderGuardrailMetric(p) },
			expectedTables:  []string{"delta.exposures", "delta.metric_events"},
			expectedColumns: []string{"user_id", "variant_id", "experiment_id", "event_type"},
		},
		{
			name: "interleaving score references exposures with provenance + metric_events",
			params: spark.TemplateParams{
				ExperimentID: "e0000000-0000-0000-0000-000000000003",
				CreditAssignment: "proportional", EngagementEventType: "click",
				ComputationDate: compDate,
			},
			render:          func(r *spark.SQLRenderer, p spark.TemplateParams) (string, error) { return r.RenderInterleavingScore(p) },
			expectedTables:  []string{"delta.exposures", "delta.metric_events"},
			expectedColumns: []string{"user_id", "experiment_id", "interleaving_provenance", "content_id"},
		},
		{
			name: "content consumption references exposures + metric_events",
			params: spark.TemplateParams{
				ExperimentID: expID, ContentIDField: "content_id",
				ComputationDate: compDate,
			},
			render:          func(r *spark.SQLRenderer, p spark.TemplateParams) (string, error) { return r.RenderContentConsumption(p) },
			expectedTables:  []string{"delta.exposures", "delta.metric_events"},
			expectedColumns: []string{"user_id", "variant_id", "experiment_id", "content_id"},
		},
		{
			name: "daily treatment effect references metric_summaries",
			params: spark.TemplateParams{
				ExperimentID: expID, MetricID: "watch_time_minutes",
				ControlVariantID: "f0000000-0000-0000-0000-000000000001",
				ComputationDate: compDate,
			},
			render:          func(r *spark.SQLRenderer, p spark.TemplateParams) (string, error) { return r.RenderDailyTreatmentEffect(p) },
			expectedTables:  []string{"delta.exposures", "delta.metric_summaries"},
			expectedColumns: []string{"user_id", "variant_id", "experiment_id", "metric_id", "metric_value"},
		},
	}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			sql, err := tc.render(renderer, tc.params)
			require.NoError(t, err, "template should render without error")
			assert.NotEmpty(t, sql, "rendered SQL should not be empty")

			// Verify expected Delta Lake tables are referenced.
			for _, table := range tc.expectedTables {
				assert.Contains(t, sql, table,
					"SQL should reference %s (M2 Delta Lake table)", table)
			}

			// Verify expected columns are referenced.
			for _, col := range tc.expectedColumns {
				assert.Contains(t, sql, col,
					"SQL should reference column %q (M2 event schema)", col)
			}

			// Verify experiment_id filter is present (all queries must scope to an experiment).
			assert.Contains(t, sql, tc.params.ExperimentID,
				"SQL should filter by experiment_id")
		})
	}
}

// TestSQLTemplateNoUnknownDeltaTables verifies that SQL templates only
// reference known Delta Lake tables, not tables that don't exist in M2's
// schema.
func TestSQLTemplateNoUnknownDeltaTables(t *testing.T) {
	renderer, err := spark.NewSQLRenderer()
	require.NoError(t, err)

	knownTables := map[string]bool{
		"delta.exposures":              true,
		"delta.metric_events":          true,
		"delta.qoe_events":             true,
		"delta.reward_events":          true,
		"delta.metric_summaries":       true,
		"delta.interleaving_scores":    true,
		"delta.content_consumption":    true,
		"delta.daily_treatment_effects": true,
	}

	// Re-used regex to find "delta.<name>" references in SQL.
	deltaTableRe := regexp.MustCompile(`delta\.(\w+)`)

	// Render every template type with valid params and check references.
	templates := []struct {
		name   string
		render func() (string, error)
	}{
		{"mean", func() (string, error) {
			return renderer.RenderMean(spark.TemplateParams{ExperimentID: "x", MetricID: "m", SourceEventType: "evt", ComputationDate: "2024-01-01"})
		}},
		{"proportion", func() (string, error) {
			return renderer.RenderProportion(spark.TemplateParams{ExperimentID: "x", MetricID: "m", SourceEventType: "evt", ComputationDate: "2024-01-01"})
		}},
		{"count", func() (string, error) {
			return renderer.RenderCount(spark.TemplateParams{ExperimentID: "x", MetricID: "m", SourceEventType: "evt", ComputationDate: "2024-01-01"})
		}},
		{"ratio", func() (string, error) {
			return renderer.RenderRatio(spark.TemplateParams{ExperimentID: "x", MetricID: "m", NumeratorEventType: "n", DenominatorEventType: "d", ComputationDate: "2024-01-01"})
		}},
		{"ratio_delta_method", func() (string, error) {
			return renderer.RenderRatioDeltaMethod(spark.TemplateParams{ExperimentID: "x", MetricID: "m", NumeratorEventType: "n", DenominatorEventType: "d", ComputationDate: "2024-01-01"})
		}},
		{"cuped_covariate", func() (string, error) {
			return renderer.RenderCupedCovariate(spark.TemplateParams{ExperimentID: "x", MetricID: "m", CupedCovariateEventType: "evt", ExperimentStartDate: "2024-01-01", CupedLookbackDays: 7, ComputationDate: "2024-01-08"})
		}},
		{"qoe_metric", func() (string, error) {
			return renderer.RenderQoEMetric(spark.TemplateParams{ExperimentID: "x", MetricID: "m", QoEField: "time_to_first_frame_ms", ComputationDate: "2024-01-01"})
		}},
		{"guardrail_metric", func() (string, error) {
			return renderer.RenderGuardrailMetric(spark.TemplateParams{ExperimentID: "x", MetricID: "m", SourceEventType: "evt", ComputationDate: "2024-01-01"})
		}},
		{"interleaving_score", func() (string, error) {
			return renderer.RenderInterleavingScore(spark.TemplateParams{ExperimentID: "x", CreditAssignment: "proportional", EngagementEventType: "click", ComputationDate: "2024-01-01"})
		}},
		{"content_consumption", func() (string, error) {
			return renderer.RenderContentConsumption(spark.TemplateParams{ExperimentID: "x", ContentIDField: "content_id", ComputationDate: "2024-01-01"})
		}},
		{"daily_treatment_effect", func() (string, error) {
			return renderer.RenderDailyTreatmentEffect(spark.TemplateParams{ExperimentID: "x", MetricID: "m", ControlVariantID: "c", ComputationDate: "2024-01-01"})
		}},
	}

	for _, tmpl := range templates {
		t.Run(tmpl.name, func(t *testing.T) {
			sql, err := tmpl.render()
			require.NoError(t, err)

			matches := deltaTableRe.FindAllStringSubmatch(sql, -1)
			for _, match := range matches {
				fullRef := "delta." + match[1]
				assert.True(t, knownTables[fullRef],
					"template %q references unknown Delta table %q — this is a schema contract violation with M2",
					tmpl.name, fullRef)
			}
		})
	}
}

// TestQoEFieldMapsToM2Schema verifies that QoE field references in SQL
// templates correspond to actual columns in delta.qoe_events (produced by M2).
func TestQoEFieldMapsToM2Schema(t *testing.T) {
	renderer, err := spark.NewSQLRenderer()
	require.NoError(t, err)

	// These are the PlaybackMetrics fields from M2's QoEEvent proto.
	validQoEFields := map[string]bool{
		"time_to_first_frame_ms": true,
		"rebuffer_count":         true,
		"rebuffer_ratio":         true,
		"avg_bitrate_kbps":       true,
		"resolution_switches":    true,
		"peak_resolution_height": true,
		"startup_failure_rate":   true,
		"playback_duration_ms":   true,
	}

	for field := range validQoEFields {
		t.Run(field, func(t *testing.T) {
			sql, err := renderer.RenderQoEMetric(spark.TemplateParams{
				ExperimentID:    "x",
				MetricID:        "test-" + field,
				QoEField:        field,
				ComputationDate: "2024-01-01",
			})
			require.NoError(t, err)
			assert.Contains(t, sql, "qe."+field,
				"QoE template should reference qe.%s from delta.qoe_events", field)
			assert.Contains(t, sql, "delta.qoe_events",
				"QoE template should read from delta.qoe_events (M2 Kafka Connect sink)")
		})
	}
}

// ---------------------------------------------------------------------------
// Test: Guardrail alert schema contract (M3 → M5)
// ---------------------------------------------------------------------------

func TestGuardrailAlertSchemaContract(t *testing.T) {
	// Verify the GuardrailAlert JSON structure matches what M5 expects.
	alert := alerts.GuardrailAlert{
		ExperimentID:           "e0000000-0000-0000-0000-000000000001",
		MetricID:               "rebuffer_rate",
		VariantID:              "f0000000-0000-0000-0000-000000000002",
		CurrentValue:           0.08,
		Threshold:              0.05,
		ConsecutiveBreachCount: 3,
	}

	data, err := json.Marshal(alert)
	require.NoError(t, err)

	// Parse back and verify all required fields are present (M5 contract).
	var parsed map[string]any
	require.NoError(t, json.Unmarshal(data, &parsed))

	// M5's guardrail processor expects these fields:
	requiredFields := []string{
		"experiment_id",
		"metric_id",
		"variant_id",
		"current_value",
		"threshold",
		"consecutive_breach_count",
	}
	for _, field := range requiredFields {
		_, ok := parsed[field]
		assert.True(t, ok, "GuardrailAlert JSON must include %q (M5 contract)", field)
	}

	// Verify field types.
	assert.IsType(t, "", parsed["experiment_id"])
	assert.IsType(t, "", parsed["metric_id"])
	assert.IsType(t, "", parsed["variant_id"])
	assert.IsType(t, float64(0), parsed["current_value"])
	assert.IsType(t, float64(0), parsed["threshold"])
	assert.IsType(t, float64(0), parsed["consecutive_breach_count"]) // JSON numbers are float64
}

func TestGuardrailBreachProducesAlert(t *testing.T) {
	cfgStore, err := config.LoadFromFile("config/testdata/seed_config.json")
	require.NoError(t, err)
	renderer, err := spark.NewSQLRenderer()
	require.NoError(t, err)
	executor := spark.NewMockExecutor(500)
	qlWriter := querylog.NewMemWriter()
	publisher := alerts.NewMemPublisher()
	tracker := alerts.NewBreachTracker()

	// Set rebuffer_rate above threshold (0.05) for the treatment variant.
	vp := jobs.NewMockValueProvider()
	vp.SetVariantValue("rebuffer_rate", "f0000000-0000-0000-0000-000000000001", 0.02) // control: below threshold
	vp.SetVariantValue("rebuffer_rate", "f0000000-0000-0000-0000-000000000002", 0.08) // treatment: above threshold
	vp.SetVariantValue("error_rate", "f0000000-0000-0000-0000-000000000001", 0.005)
	vp.SetVariantValue("error_rate", "f0000000-0000-0000-0000-000000000002", 0.005)

	gj := jobs.NewGuardrailJob(cfgStore, renderer, executor, qlWriter, publisher, tracker, vp)
	ctx := context.Background()
	expID := "e0000000-0000-0000-0000-000000000001"

	// rebuffer_rate requires 3 consecutive breaches.
	for i := 0; i < 3; i++ {
		result, err := gj.Run(ctx, expID)
		require.NoError(t, err)
		assert.Equal(t, 2, result.GuardrailsChecked) // rebuffer_rate + error_rate
	}

	// After 3 consecutive breaches, an alert should have been published.
	publishedAlerts := publisher.Alerts()
	require.True(t, len(publishedAlerts) > 0, "should publish at least 1 guardrail alert after consecutive breaches")

	// Verify the alert targets the correct experiment and metric.
	found := false
	for _, a := range publishedAlerts {
		if a.ExperimentID == expID && a.MetricID == "rebuffer_rate" &&
			a.VariantID == "f0000000-0000-0000-0000-000000000002" {
			found = true
			assert.Equal(t, 0.08, a.CurrentValue)
			assert.Equal(t, 0.05, a.Threshold)
			assert.GreaterOrEqual(t, a.ConsecutiveBreachCount, 3)
			break
		}
	}
	assert.True(t, found, "expected alert for rebuffer_rate breach on treatment variant")
}

// ---------------------------------------------------------------------------
// Test: Multi-experiment query_log isolation
// ---------------------------------------------------------------------------

func TestQueryLogExperimentIsolation(t *testing.T) {
	env := setupIntegration(t)
	ctx := context.Background()

	// Create two experiments.
	exp1 := uuid.New()
	exp2 := uuid.New()
	layerID := uuid.New()

	_, err := env.pool.Exec(ctx,
		`INSERT INTO layers (layer_id, name) VALUES ($1, $2) ON CONFLICT (name) DO NOTHING`,
		layerID, fmt.Sprintf("iso-layer-%s", layerID))
	require.NoError(t, err)

	for _, eid := range []uuid.UUID{exp1, exp2} {
		_, err = env.pool.Exec(ctx,
			`INSERT INTO experiments (experiment_id, name, owner_email, type, state, layer_id, primary_metric_id)
			 VALUES ($1, $2, $3, $4, $5, $6, $7)`,
			eid, fmt.Sprintf("iso-exp-%s", eid), "test@example.com", "AB", "RUNNING", layerID, "watch_time_minutes")
		require.NoError(t, err)
	}
	t.Cleanup(func() {
		for _, eid := range []uuid.UUID{exp1, exp2} {
			_, _ = env.pool.Exec(context.Background(), `DELETE FROM query_log WHERE experiment_id = $1`, eid)
			_, _ = env.pool.Exec(context.Background(), `DELETE FROM experiments WHERE experiment_id = $1`, eid)
		}
		_, _ = env.pool.Exec(context.Background(), `DELETE FROM layers WHERE layer_id = $1`, layerID)
	})

	// Log entries for each experiment.
	for i, eid := range []uuid.UUID{exp1, exp2} {
		for j := 0; j < i+1; j++ {
			err := env.pgWriter.Log(ctx, querylog.Entry{
				ExperimentID: eid.String(),
				MetricID:     "metric_a",
				SQLText:      fmt.Sprintf("SELECT %d FROM test", j),
				RowCount:     int64(j),
				DurationMs:   10,
				JobType:      "daily_metric",
			})
			require.NoError(t, err)
		}
	}

	// exp1 should have 1 entry, exp2 should have 2.
	entries1, err := env.pgWriter.GetLogs(ctx, exp1.String(), "")
	require.NoError(t, err)
	assert.Len(t, entries1, 1)

	entries2, err := env.pgWriter.GetLogs(ctx, exp2.String(), "")
	require.NoError(t, err)
	assert.Len(t, entries2, 2)
}

// ---------------------------------------------------------------------------
// Test: SQL templates produce valid SQL (no template rendering errors)
// ---------------------------------------------------------------------------

func TestAllSQLTemplatesRenderWithoutError(t *testing.T) {
	renderer, err := spark.NewSQLRenderer()
	require.NoError(t, err)

	// Render every template type with minimal valid params.
	// This catches template syntax errors and missing field references.
	type renderCase struct {
		name   string
		render func() (string, error)
	}
	cases := []renderCase{
		{"mean", func() (string, error) {
			return renderer.RenderMean(spark.TemplateParams{ExperimentID: "x", MetricID: "m", SourceEventType: "e", ComputationDate: "2024-01-01"})
		}},
		{"proportion", func() (string, error) {
			return renderer.RenderProportion(spark.TemplateParams{ExperimentID: "x", MetricID: "m", SourceEventType: "e", ComputationDate: "2024-01-01"})
		}},
		{"count", func() (string, error) {
			return renderer.RenderCount(spark.TemplateParams{ExperimentID: "x", MetricID: "m", SourceEventType: "e", ComputationDate: "2024-01-01"})
		}},
		{"ratio", func() (string, error) {
			return renderer.RenderRatio(spark.TemplateParams{ExperimentID: "x", MetricID: "m", NumeratorEventType: "n", DenominatorEventType: "d", ComputationDate: "2024-01-01"})
		}},
		{"ratio_delta_method", func() (string, error) {
			return renderer.RenderRatioDeltaMethod(spark.TemplateParams{ExperimentID: "x", MetricID: "m", NumeratorEventType: "n", DenominatorEventType: "d", ComputationDate: "2024-01-01"})
		}},
		{"cuped_covariate", func() (string, error) {
			return renderer.RenderCupedCovariate(spark.TemplateParams{ExperimentID: "x", MetricID: "m", CupedCovariateEventType: "e", ExperimentStartDate: "2024-01-01", CupedLookbackDays: 7, ComputationDate: "2024-01-08"})
		}},
		{"qoe_metric", func() (string, error) {
			return renderer.RenderQoEMetric(spark.TemplateParams{ExperimentID: "x", MetricID: "m", QoEField: "time_to_first_frame_ms", ComputationDate: "2024-01-01"})
		}},
		{"guardrail_metric", func() (string, error) {
			return renderer.RenderGuardrailMetric(spark.TemplateParams{ExperimentID: "x", MetricID: "m", SourceEventType: "e", ComputationDate: "2024-01-01"})
		}},
		{"content_consumption", func() (string, error) {
			return renderer.RenderContentConsumption(spark.TemplateParams{ExperimentID: "x", ContentIDField: "content_id", ComputationDate: "2024-01-01"})
		}},
		{"daily_treatment_effect", func() (string, error) {
			return renderer.RenderDailyTreatmentEffect(spark.TemplateParams{ExperimentID: "x", MetricID: "m", ControlVariantID: "c", ComputationDate: "2024-01-01"})
		}},
		{"lifecycle_mean", func() (string, error) {
			return renderer.RenderLifecycleMean(spark.TemplateParams{ExperimentID: "x", MetricID: "m", SourceEventType: "e", ComputationDate: "2024-01-01", LifecycleEnabled: true})
		}},
		{"session_level_mean", func() (string, error) {
			return renderer.RenderSessionLevelMean(spark.TemplateParams{ExperimentID: "x", MetricID: "m", SourceEventType: "e", ComputationDate: "2024-01-01", SessionLevel: true})
		}},
		{"surrogate_input", func() (string, error) {
			return renderer.RenderSurrogateInput(spark.TemplateParams{ExperimentID: "x", InputMetricIDs: []string{"a", "b"}, ObservationWindowDays: 7, ComputationDate: "2024-01-01"})
		}},
		{"interleaving_score_binary_win", func() (string, error) {
			return renderer.RenderInterleavingScore(spark.TemplateParams{ExperimentID: "x", CreditAssignment: "binary_win", EngagementEventType: "click", ComputationDate: "2024-01-01"})
		}},
		{"interleaving_score_proportional", func() (string, error) {
			return renderer.RenderInterleavingScore(spark.TemplateParams{ExperimentID: "x", CreditAssignment: "proportional", EngagementEventType: "click", ComputationDate: "2024-01-01"})
		}},
		{"interleaving_score_weighted", func() (string, error) {
			return renderer.RenderInterleavingScore(spark.TemplateParams{ExperimentID: "x", CreditAssignment: "weighted", EngagementEventType: "click", ComputationDate: "2024-01-01"})
		}},
		{"qoe_engagement_correlation", func() (string, error) {
			return renderer.RenderQoEEngagementCorrelation(spark.TemplateParams{ExperimentID: "x", QoEFieldA: "time_to_first_frame_ms", EngagementSourceType: "heartbeat", ComputationDate: "2024-01-01"})
		}},
	}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			sql, err := tc.render()
			require.NoError(t, err, "template %q should render without error", tc.name)
			assert.NotEmpty(t, sql, "rendered SQL should not be empty")
			// Basic SQL sanity: should contain SELECT.
			assert.Contains(t, strings.ToUpper(sql), "SELECT",
				"rendered SQL should contain a SELECT statement")
		})
	}
}
