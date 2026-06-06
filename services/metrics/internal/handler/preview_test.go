package handler

import (
	"context"
	"errors"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"connectrpc.com/connect"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"

	commonv1 "github.com/org/experimentation/gen/go/experimentation/common/v1"
	metricsv1 "github.com/org/experimentation/gen/go/experimentation/metrics/v1"
	"github.com/org/experimentation/gen/go/experimentation/metrics/v1/metricsv1connect"

	"github.com/org/experimentation-platform/services/metrics/internal/querylog"
)

// stubCatalogReader is a test-local catalog.CatalogReader that returns a
// canned slice of metric IDs. Test-local on purpose — we don't want a mock
// living in the production `catalog` package, and the real PgPoolCatalog is
// covered by integration tests rather than unit tests against a real
// Postgres.
type stubCatalogReader struct {
	ids []string
	err error
}

func (s *stubCatalogReader) ListMetricIDs(context.Context) ([]string, error) {
	if s.err != nil {
		return nil, s.err
	}
	return s.ids, nil
}

// setupPreviewClient constructs a minimal MetricsHandler — only
// CompileMetricqlPreview is exercised here, so all job dependencies are nil.
func setupPreviewClient(t *testing.T) metricsv1connect.MetricComputationServiceClient {
	t.Helper()
	ql := querylog.NewMemWriter()
	h := NewMetricsHandler(nil, nil, nil, nil, nil, nil, ql)
	mux := http.NewServeMux()
	path, hnd := metricsv1connect.NewMetricComputationServiceHandler(h)
	mux.Handle(path, hnd)
	srv := httptest.NewServer(mux)
	t.Cleanup(srv.Close)
	return metricsv1connect.NewMetricComputationServiceClient(http.DefaultClient, srv.URL)
}

// setupPreviewClientWithCatalog constructs a handler with a stub
// catalog.CatalogReader wired via WithCatalogReader, returning the canned
// list of known metric IDs. Used by the Issue #597 global-scope tests.
func setupPreviewClientWithCatalog(t *testing.T, knownIDs []string) metricsv1connect.MetricComputationServiceClient {
	t.Helper()
	ql := querylog.NewMemWriter()
	h := NewMetricsHandler(nil, nil, nil, nil, nil, nil, ql, WithCatalogReader(&stubCatalogReader{ids: knownIDs}))
	mux := http.NewServeMux()
	path, hnd := metricsv1connect.NewMetricComputationServiceHandler(h)
	mux.Handle(path, hnd)
	srv := httptest.NewServer(mux)
	t.Cleanup(srv.Close)
	return metricsv1connect.NewMetricComputationServiceClient(http.DefaultClient, srv.URL)
}

// TestCompileMetricqlPreview_EmptyExperimentId verifies the Issue #597
// relaxation: an empty experiment_id is now treated as GLOBAL scope. With a
// catalogReader wired and "watch_time" in the catalog, a plain aggregation
// expression with no @metric_refs should succeed with empty diagnostics.
func TestCompileMetricqlPreview_EmptyExperimentId(t *testing.T) {
	client := setupPreviewClientWithCatalog(t, []string{"watch_time"})
	resp, err := client.CompileMetricqlPreview(context.Background(), connect.NewRequest(&metricsv1.CompileMetricqlPreviewRequest{
		ExperimentId:       "",
		MetricqlExpression: "mean(heartbeat.value)",
	}))
	require.NoError(t, err)
	assert.Empty(t, resp.Msg.GetDiagnostics())
	assert.NotEmpty(t, resp.Msg.GetCompiledSql())
}

// TestCompileMetricqlPreview_EmptyExperimentId_UnknownRefReturnsDiagnostic
// verifies that with the catalog populated to {known_metric}, an expression
// referencing @unknown_metric returns 200 OK with one SEVERITY_ERROR
// diagnostic whose message mentions "unknown_metric".
func TestCompileMetricqlPreview_EmptyExperimentId_UnknownRefReturnsDiagnostic(t *testing.T) {
	client := setupPreviewClientWithCatalog(t, []string{"known_metric"})
	resp, err := client.CompileMetricqlPreview(context.Background(), connect.NewRequest(&metricsv1.CompileMetricqlPreviewRequest{
		ExperimentId:       "",
		MetricqlExpression: "@unknown_metric + 0",
	}))
	require.NoError(t, err) // handler returns 200 with diagnostics, not a gRPC error
	assert.Empty(t, resp.Msg.GetCompiledSql())
	require.Len(t, resp.Msg.GetDiagnostics(), 1)
	d := resp.Msg.GetDiagnostics()[0]
	assert.Equal(t, commonv1.MetricqlDiagnostic_SEVERITY_ERROR, d.GetSeverity())
	assert.Contains(t, d.GetMessage(), "unknown_metric")
}

// TestCompileMetricqlPreview_EmptyExperimentId_KnownRefReturnsSql verifies
// that with the catalog containing the referenced metric, the expression
// compiles to SQL with no diagnostics.
func TestCompileMetricqlPreview_EmptyExperimentId_KnownRefReturnsSql(t *testing.T) {
	client := setupPreviewClientWithCatalog(t, []string{"watch_time"})
	resp, err := client.CompileMetricqlPreview(context.Background(), connect.NewRequest(&metricsv1.CompileMetricqlPreviewRequest{
		ExperimentId:       "",
		MetricqlExpression: "@watch_time + 0",
	}))
	require.NoError(t, err)
	assert.Empty(t, resp.Msg.GetDiagnostics())
	assert.NotEmpty(t, resp.Msg.GetCompiledSql())
}

// TestCompileMetricqlPreview_EmptyExperimentId_NoCatalogReader verifies
// the legacy backward-compat path: when no WithCatalogReader option is
// passed, an empty experiment_id still succeeds — but with KnownMetricIDs
// effectively nil, so even unknown @metric_refs are accepted (existence
// check skipped per the analyzer's nil-means-skip contract).
func TestCompileMetricqlPreview_EmptyExperimentId_NoCatalogReader(t *testing.T) {
	client := setupPreviewClient(t) // no WithCatalogReader option
	resp, err := client.CompileMetricqlPreview(context.Background(), connect.NewRequest(&metricsv1.CompileMetricqlPreviewRequest{
		ExperimentId:       "",
		MetricqlExpression: "@unknown_metric + 0",
	}))
	require.NoError(t, err)
	// Skip-existence-check path: no unknown-reference diagnostic, and the
	// expression compiles (the analyzer accepts the @metric_ref because
	// KnownMetricIDs is nil).
	assert.Empty(t, resp.Msg.GetDiagnostics())
	assert.NotEmpty(t, resp.Msg.GetCompiledSql())
}

// TestCompileMetricqlPreview_EmptyExperimentId_CatalogError_ReturnsInternal
// verifies that when the catalog reader fails (e.g. DB unavailable), the
// handler surfaces CodeInternal rather than silently falling through to
// the skip-existence-check path. The fallthrough would mask unknown
// @metric_refs from the operator, so a hard error is the correct signal.
func TestCompileMetricqlPreview_EmptyExperimentId_CatalogError_ReturnsInternal(t *testing.T) {
	t.Helper()
	ql := querylog.NewMemWriter()
	h := NewMetricsHandler(nil, nil, nil, nil, nil, nil, ql,
		WithCatalogReader(&stubCatalogReader{err: errors.New("db down")}))
	mux := http.NewServeMux()
	path, hnd := metricsv1connect.NewMetricComputationServiceHandler(h)
	mux.Handle(path, hnd)
	srv := httptest.NewServer(mux)
	t.Cleanup(srv.Close)
	client := metricsv1connect.NewMetricComputationServiceClient(http.DefaultClient, srv.URL)

	_, err := client.CompileMetricqlPreview(context.Background(), connect.NewRequest(&metricsv1.CompileMetricqlPreviewRequest{
		ExperimentId:       "",
		MetricqlExpression: "mean(heartbeat.value)",
	}))
	require.Error(t, err)
	assert.Equal(t, connect.CodeInternal, connect.CodeOf(err))
}

// TestCompileMetricqlPreview_WhitespaceOnlyExperimentId_TreatedAsGlobal
// verifies symmetry with PR #595 / Task 1's M5 normalization: an
// experiment_id consisting only of whitespace is treated identically to
// the empty string (i.e. as GLOBAL scope).
func TestCompileMetricqlPreview_WhitespaceOnlyExperimentId_TreatedAsGlobal(t *testing.T) {
	client := setupPreviewClientWithCatalog(t, []string{"known_metric"})
	resp, err := client.CompileMetricqlPreview(context.Background(), connect.NewRequest(&metricsv1.CompileMetricqlPreviewRequest{
		ExperimentId:       "   \t  ",
		MetricqlExpression: "@unknown_metric + 0",
	}))
	require.NoError(t, err)
	// Whitespace-only experiment_id is normalized to global scope, so the
	// stub catalog applies and @unknown_metric is flagged.
	assert.Empty(t, resp.Msg.GetCompiledSql())
	require.Len(t, resp.Msg.GetDiagnostics(), 1)
	d := resp.Msg.GetDiagnostics()[0]
	assert.Equal(t, commonv1.MetricqlDiagnostic_SEVERITY_ERROR, d.GetSeverity())
	assert.Contains(t, d.GetMessage(), "unknown_metric")
}

func TestCompileMetricqlPreview_EmptyExpression(t *testing.T) {
	client := setupPreviewClient(t)
	resp, err := client.CompileMetricqlPreview(context.Background(), connect.NewRequest(&metricsv1.CompileMetricqlPreviewRequest{
		ExperimentId:       "exp-1",
		MetricqlExpression: "",
	}))
	require.NoError(t, err)
	assert.Empty(t, resp.Msg.GetCompiledSql())
	require.Len(t, resp.Msg.GetDiagnostics(), 1)
	assert.Contains(t, resp.Msg.GetDiagnostics()[0].GetMessage(), "empty")
	assert.Equal(t, commonv1.MetricqlDiagnostic_SEVERITY_ERROR, resp.Msg.GetDiagnostics()[0].GetSeverity())
}

func TestCompileMetricqlPreview_WhitespaceOnlyExpression(t *testing.T) {
	client := setupPreviewClient(t)
	resp, err := client.CompileMetricqlPreview(context.Background(), connect.NewRequest(&metricsv1.CompileMetricqlPreviewRequest{
		ExperimentId:       "exp-1",
		MetricqlExpression: "   \t\n  ",
	}))
	require.NoError(t, err)
	assert.Empty(t, resp.Msg.GetCompiledSql())
	require.Len(t, resp.Msg.GetDiagnostics(), 1)
	assert.Contains(t, resp.Msg.GetDiagnostics()[0].GetMessage(), "empty")
}

func TestCompileMetricqlPreview_ValidExpressionProducesSql(t *testing.T) {
	client := setupPreviewClient(t)
	resp, err := client.CompileMetricqlPreview(context.Background(), connect.NewRequest(&metricsv1.CompileMetricqlPreviewRequest{
		ExperimentId:       "exp-1",
		MetricqlExpression: "mean(heartbeat.value)",
	}))
	require.NoError(t, err)
	assert.Empty(t, resp.Msg.GetDiagnostics())
	require.NotEmpty(t, resp.Msg.GetCompiledSql())
	// The aggregation template emits AVG (or MEAN via codegen). Either form
	// is acceptable -- check case-insensitively for the source field name.
	assert.Contains(t, strings.ToLower(resp.Msg.GetCompiledSql()), "heartbeat")
}

func TestCompileMetricqlPreview_ParseErrorReturnsDiagnostic(t *testing.T) {
	client := setupPreviewClient(t)
	// Unclosed paren is a parse error.
	resp, err := client.CompileMetricqlPreview(context.Background(), connect.NewRequest(&metricsv1.CompileMetricqlPreviewRequest{
		ExperimentId:       "exp-1",
		MetricqlExpression: "mean(heartbeat.value", // missing ')'
	}))
	require.NoError(t, err) // handler returns 200 with diagnostics, not a gRPC error
	assert.Empty(t, resp.Msg.GetCompiledSql())
	require.Len(t, resp.Msg.GetDiagnostics(), 1)
	assert.Equal(t, commonv1.MetricqlDiagnostic_SEVERITY_ERROR, resp.Msg.GetDiagnostics()[0].GetSeverity())
	assert.NotEmpty(t, resp.Msg.GetDiagnostics()[0].GetMessage())
}

func TestCompileMetricqlPreview_LexErrorReturnsDiagnostic(t *testing.T) {
	client := setupPreviewClient(t)
	// An unclosed string literal triggers a lex error.
	resp, err := client.CompileMetricqlPreview(context.Background(), connect.NewRequest(&metricsv1.CompileMetricqlPreviewRequest{
		ExperimentId:       "exp-1",
		MetricqlExpression: "mean(event) where field = 'unclosed",
	}))
	require.NoError(t, err)
	assert.Empty(t, resp.Msg.GetCompiledSql())
	require.Len(t, resp.Msg.GetDiagnostics(), 1)
	assert.Equal(t, commonv1.MetricqlDiagnostic_SEVERITY_ERROR, resp.Msg.GetDiagnostics()[0].GetSeverity())
}

func TestCompileMetricqlPreview_AnalyzeErrorReturnsDiagnostic(t *testing.T) {
	client := setupPreviewClient(t)
	// Bare @metric_ref at top-level is a semantic (analyze) error.
	resp, err := client.CompileMetricqlPreview(context.Background(), connect.NewRequest(&metricsv1.CompileMetricqlPreviewRequest{
		ExperimentId:       "exp-1",
		MetricqlExpression: "@some_metric",
	}))
	require.NoError(t, err)
	assert.Empty(t, resp.Msg.GetCompiledSql())
	require.Len(t, resp.Msg.GetDiagnostics(), 1)
	assert.Equal(t, commonv1.MetricqlDiagnostic_SEVERITY_ERROR, resp.Msg.GetDiagnostics()[0].GetSeverity())
}

func TestCompileMetricqlPreview_DeadlineExceededShortCircuits(t *testing.T) {
	client := setupPreviewClient(t)
	// Already-expired context should fail fast with DeadlineExceeded.
	ctx, cancel := context.WithDeadline(context.Background(), time.Now().Add(-time.Millisecond))
	defer cancel()
	_, err := client.CompileMetricqlPreview(ctx, connect.NewRequest(&metricsv1.CompileMetricqlPreviewRequest{
		ExperimentId:       "exp-1",
		MetricqlExpression: "mean(heartbeat.value)",
	}))
	require.Error(t, err)
	code := connect.CodeOf(err)
	// DeadlineExceeded or Canceled — both are acceptable for an expired context.
	assert.True(t, code == connect.CodeDeadlineExceeded || code == connect.CodeCanceled,
		"expected DeadlineExceeded or Canceled, got %v", code)
}

func TestCompileMetricqlPreview_SpanAttributionOnError(t *testing.T) {
	client := setupPreviewClient(t)
	resp, err := client.CompileMetricqlPreview(context.Background(), connect.NewRequest(&metricsv1.CompileMetricqlPreviewRequest{
		ExperimentId:       "exp-1",
		MetricqlExpression: "mean(heartbeat.value", // parse error
	}))
	require.NoError(t, err)
	require.Len(t, resp.Msg.GetDiagnostics(), 1)
	span := resp.Msg.GetDiagnostics()[0].GetSpan()
	require.NotNil(t, span)
	// Line must be 1-based (>= 1).
	assert.GreaterOrEqual(t, span.GetLine(), int32(1))
	// Column must be 1-based (>= 1).
	assert.GreaterOrEqual(t, span.GetColumn(), int32(1))
}

func TestCompileMetricqlPreview_LineColAttributionMultiLine(t *testing.T) {
	client := setupPreviewClient(t)
	// Line 1 is valid mean; line 2 has the error (missing operand after +).
	expr := "mean(heartbeat.value)\n+ bad_identifier_not_a_keyword"
	resp, err := client.CompileMetricqlPreview(context.Background(), connect.NewRequest(&metricsv1.CompileMetricqlPreviewRequest{
		ExperimentId:       "exp-1",
		MetricqlExpression: expr,
	}))
	require.NoError(t, err)
	require.NotEmpty(t, resp.Msg.GetDiagnostics())
	// At least one diagnostic should be on or after offset of '\n' (offset 21),
	// which corresponds to line 2.
	foundLine2OrHigher := false
	for _, d := range resp.Msg.GetDiagnostics() {
		if d.GetSpan() != nil && d.GetSpan().GetLine() >= 2 {
			foundLine2OrHigher = true
		}
	}
	assert.True(t, foundLine2OrHigher,
		"expected at least one diagnostic on line 2+, got: %+v", resp.Msg.GetDiagnostics())
}

func TestCompileMetricqlPreview_SumExpression(t *testing.T) {
	client := setupPreviewClient(t)
	resp, err := client.CompileMetricqlPreview(context.Background(), connect.NewRequest(&metricsv1.CompileMetricqlPreviewRequest{
		ExperimentId:       "exp-1",
		MetricqlExpression: "sum(purchase.amount)",
	}))
	require.NoError(t, err)
	assert.Empty(t, resp.Msg.GetDiagnostics())
	require.NotEmpty(t, resp.Msg.GetCompiledSql())
	assert.Contains(t, strings.ToLower(resp.Msg.GetCompiledSql()), "purchase")
}

func TestCompileMetricqlPreview_CountExpression(t *testing.T) {
	client := setupPreviewClient(t)
	resp, err := client.CompileMetricqlPreview(context.Background(), connect.NewRequest(&metricsv1.CompileMetricqlPreviewRequest{
		ExperimentId:       "exp-1",
		MetricqlExpression: "count(stream_start)",
	}))
	require.NoError(t, err)
	assert.Empty(t, resp.Msg.GetDiagnostics())
	require.NotEmpty(t, resp.Msg.GetCompiledSql())
}
