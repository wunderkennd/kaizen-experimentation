package spark

import (
	"context"
	"sync"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func TestMockExecutor_ExecuteSQL(t *testing.T) {
	exec := NewMockExecutor(42)
	result, err := exec.ExecuteSQL(context.Background(), "SELECT 1")
	require.NoError(t, err)
	assert.Equal(t, int64(42), result.RowCount)
	assert.NotZero(t, result.Duration)

	calls := exec.GetCalls()
	require.Len(t, calls, 1)
	assert.Equal(t, "SELECT 1", calls[0].SQL)
	assert.Empty(t, calls[0].TargetTable)
}

func TestMockExecutor_ExecuteAndWrite(t *testing.T) {
	exec := NewMockExecutor(100)
	result, err := exec.ExecuteAndWrite(context.Background(), "INSERT INTO t", "delta.metric_summaries")
	require.NoError(t, err)
	assert.Equal(t, int64(100), result.RowCount)

	calls := exec.GetCalls()
	require.Len(t, calls, 1)
	assert.Equal(t, "INSERT INTO t", calls[0].SQL)
	assert.Equal(t, "delta.metric_summaries", calls[0].TargetTable)
}

func TestMockExecutor_MultipleCalls(t *testing.T) {
	exec := NewMockExecutor(10)
	_, _ = exec.ExecuteSQL(context.Background(), "SELECT 1")
	_, _ = exec.ExecuteSQL(context.Background(), "SELECT 2")
	_, _ = exec.ExecuteAndWrite(context.Background(), "INSERT", "table_a")

	calls := exec.GetCalls()
	require.Len(t, calls, 3)
	assert.Equal(t, "SELECT 1", calls[0].SQL)
	assert.Equal(t, "SELECT 2", calls[1].SQL)
	assert.Equal(t, "INSERT", calls[2].SQL)
	assert.Equal(t, "table_a", calls[2].TargetTable)
}

func TestMockExecutor_Reset(t *testing.T) {
	exec := NewMockExecutor(10)
	_, _ = exec.ExecuteSQL(context.Background(), "SELECT 1")
	_, _ = exec.ExecuteSQL(context.Background(), "SELECT 2")
	assert.Len(t, exec.GetCalls(), 2)

	exec.Reset()
	assert.Empty(t, exec.GetCalls())

	// Can continue recording after reset.
	_, _ = exec.ExecuteSQL(context.Background(), "SELECT 3")
	calls := exec.GetCalls()
	require.Len(t, calls, 1)
	assert.Equal(t, "SELECT 3", calls[0].SQL)
}

func TestMockExecutor_GetCalls_ReturnsCopy(t *testing.T) {
	exec := NewMockExecutor(10)
	_, _ = exec.ExecuteSQL(context.Background(), "SELECT 1")

	calls1 := exec.GetCalls()
	calls2 := exec.GetCalls()
	require.Len(t, calls1, 1)
	require.Len(t, calls2, 1)

	// Mutating the returned slice should not affect the executor's internal state.
	calls1[0].SQL = "MODIFIED"
	assert.Equal(t, "SELECT 1", exec.GetCalls()[0].SQL)
}

func TestMockExecutor_ConcurrentAccess(t *testing.T) {
	exec := NewMockExecutor(1)
	var wg sync.WaitGroup
	const goroutines = 50

	wg.Add(goroutines)
	for i := 0; i < goroutines; i++ {
		go func() {
			defer wg.Done()
			_, _ = exec.ExecuteSQL(context.Background(), "SELECT 1")
			_ = exec.GetCalls()
		}()
	}
	wg.Wait()

	assert.Len(t, exec.GetCalls(), goroutines)
}

func TestMockExecutor_ZeroRowCount(t *testing.T) {
	exec := NewMockExecutor(0)
	result, err := exec.ExecuteSQL(context.Background(), "SELECT 1")
	require.NoError(t, err)
	assert.Equal(t, int64(0), result.RowCount)
}

func TestNewSQLRenderer_Render_InvalidTemplate(t *testing.T) {
	r, err := NewSQLRenderer()
	require.NoError(t, err)

	_, err = r.Render("nonexistent_template.sql.tmpl", TemplateParams{})
	require.Error(t, err)
	assert.Contains(t, err.Error(), "spark: render nonexistent_template.sql.tmpl")
}

func TestRenderForType_AllCases(t *testing.T) {
	r, err := NewSQLRenderer()
	require.NoError(t, err)

	p := TemplateParams{
		ExperimentID:         "exp-test",
		MetricID:             "test_metric",
		SourceEventType:      "test_event",
		ComputationDate:      "2024-01-01",
		NumeratorEventType:   "num",
		DenominatorEventType: "denom",
	}

	// Valid types (case insensitive).
	for _, mt := range []string{"MEAN", "mean", "Mean", "PROPORTION", "proportion", "COUNT", "count", "RATIO", "ratio"} {
		sql, err := r.RenderForType(mt, p)
		assert.NoError(t, err, "type %q should succeed", mt)
		assert.NotEmpty(t, sql, "type %q should produce SQL", mt)
	}

	// PERCENTILE type with valid percentile value.
	pctP := p
	pctP.Percentile = 0.50
	sql, err := r.RenderForType("PERCENTILE", pctP)
	assert.NoError(t, err, "PERCENTILE with valid percentile should succeed")
	assert.NotEmpty(t, sql)
	assert.Contains(t, sql, "PERCENTILE_APPROX")

	// PERCENTILE type without valid percentile fails.
	_, err = r.RenderForType("PERCENTILE", p) // p.Percentile == 0
	assert.Error(t, err, "PERCENTILE without valid percentile should fail")
	assert.Contains(t, err.Error(), "requires percentile in (0,1)")

	// PERCENTILE case insensitive.
	pctP2 := p
	pctP2.Percentile = 0.95
	sql, err = r.RenderForType("percentile", pctP2)
	assert.NoError(t, err, "percentile (lowercase) should succeed")
	assert.Contains(t, sql, "0.95")

	// CUSTOM type with valid custom_sql.
	customP := p
	customP.CustomSQL = "SELECT user_id, AVG(value) AS metric_value FROM events GROUP BY user_id"
	sql, err = r.RenderForType("CUSTOM", customP)
	assert.NoError(t, err, "CUSTOM with valid SQL should succeed")
	assert.NotEmpty(t, sql)
	assert.Contains(t, sql, "custom_result")

	// CUSTOM type without custom_sql fails.
	_, err = r.RenderForType("CUSTOM", p)
	assert.Error(t, err, "CUSTOM without custom_sql should fail")
	assert.Contains(t, err.Error(), "requires non-empty custom_sql")

	// Invalid types.
	for _, mt := range []string{"HISTOGRAM", "", "  "} {
		_, err := r.RenderForType(mt, p)
		assert.Error(t, err, "type %q should fail", mt)
		assert.Contains(t, err.Error(), "unsupported metric type")
	}
}
