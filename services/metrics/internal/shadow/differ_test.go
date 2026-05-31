package shadow

// differ_test.go — unit tests for the B3 differ + tolerance check.
// All tests use MockValueReader (in-memory) + MockStore; no real Postgres.

import (
	"context"
	"fmt"
	"sync"
	"testing"

	"github.com/google/uuid"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

// ---------------------------------------------------------------------------
// MockValueReader
// ---------------------------------------------------------------------------

// MockValueReader is an in-memory ValueReader for unit tests.
// Pre-seed via SetValues(metricID, experimentID, computationDate, map[variantID]float64).
type MockValueReader struct {
	mu   sync.Mutex
	data map[readerKey]map[string]float64
}

type readerKey struct {
	metricID        string
	experimentID    string
	computationDate string
}

// NewMockValueReader returns an empty MockValueReader.
func NewMockValueReader() *MockValueReader {
	return &MockValueReader{data: make(map[readerKey]map[string]float64)}
}

// SetValues seeds the reader with per-variant values for a (metricID, experimentID, computationDate) triple.
func (m *MockValueReader) SetValues(metricID, experimentID, computationDate string, values map[string]float64) {
	m.mu.Lock()
	defer m.mu.Unlock()
	k := readerKey{metricID, experimentID, computationDate}
	m.data[k] = values
}

// Read implements ValueReader.
func (m *MockValueReader) Read(_ context.Context, metricID, experimentID, computationDate string) (map[string]float64, error) {
	m.mu.Lock()
	defer m.mu.Unlock()
	k := readerKey{metricID, experimentID, computationDate}
	vals, ok := m.data[k]
	if !ok {
		// Return empty map — no values for this tuple (as if the metric produced
		// no output for this experiment on this date).
		return make(map[string]float64), nil
	}
	out := make(map[string]float64, len(vals))
	for v, f := range vals {
		out[v] = f
	}
	return out, nil
}

// ---------------------------------------------------------------------------
// errValueReader — always returns an error (for failure-path tests)
// ---------------------------------------------------------------------------

type errValueReader struct{ err error }

func (e *errValueReader) Read(_ context.Context, _, _, _ string) (map[string]float64, error) {
	return nil, e.err
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

func newRunForTest(originalMetricID string) *Run {
	return &Run{
		ShadowID:         uuid.New(),
		OriginalMetricID: originalMetricID,
	}
}

const (
	testExpID  = "exp-test-001"
	testDate   = "2026-05-30"
	origMetric = "watch_time"
)

// resultRowsForShadow returns all per-variant rows (VariantID != "") for the
// given shadow from the mock store.
func resultRowsForShadow(t *testing.T, ms *MockStore, shadowID uuid.UUID) []ResultRow {
	t.Helper()
	all, err := ms.Results(context.Background(), shadowID)
	require.NoError(t, err)
	var rows []ResultRow
	for _, r := range all {
		if r.VariantID != "" {
			rows = append(rows, r)
		}
	}
	return rows
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

// TestDiffer_IdenticalValues_WithinTolerance verifies that when orig == cand
// for all variants, all ResultRows have within_tolerance = true and zero diffs.
func TestDiffer_IdenticalValues_WithinTolerance(t *testing.T) {
	ms := NewMockStore()
	reader := NewMockValueReader()
	run := newRunForTest(origMetric)

	origVals := map[string]float64{"control": 0.6, "treatment": 0.5}
	reader.SetValues(origMetric, testExpID, testDate, origVals)
	reader.SetValues(run.ShadowID.String(), testExpID, testDate, origVals)

	d := NewDiffer(reader, ms)
	require.NoError(t, d.Run(context.Background(), run, testExpID, testDate, "MEAN"))

	rows := resultRowsForShadow(t, ms, run.ShadowID)
	require.Len(t, rows, 2)
	for _, r := range rows {
		assert.True(t, r.WithinTolerance, "variant %s: expected within_tolerance=true", r.VariantID)
		assert.True(t, r.DiffAbs.Valid)
		assert.Equal(t, 0.0, r.DiffAbs.Float64, "variant %s: diff_abs must be 0", r.VariantID)
		assert.True(t, r.DiffRel.Valid)
		assert.Equal(t, 0.0, r.DiffRel.Float64, "variant %s: diff_rel must be 0", r.VariantID)
	}
}

// TestDiffer_FPDriftWithinTolerance verifies that floating-point drift smaller
// than the tolerance threshold is accepted.
//   - orig = 0.5, cand = 0.5 + 1e-12
//   - diff_rel = 1e-12 / max(0.5, 1) = 1e-12 << 1e-9
func TestDiffer_FPDriftWithinTolerance(t *testing.T) {
	ms := NewMockStore()
	reader := NewMockValueReader()
	run := newRunForTest(origMetric)

	const orig = 0.5
	const drift = 1e-12
	reader.SetValues(origMetric, testExpID, testDate, map[string]float64{"treatment": orig})
	reader.SetValues(run.ShadowID.String(), testExpID, testDate, map[string]float64{"treatment": orig + drift})

	d := NewDiffer(reader, ms)
	require.NoError(t, d.Run(context.Background(), run, testExpID, testDate, "MEAN"))

	rows := resultRowsForShadow(t, ms, run.ShadowID)
	require.Len(t, rows, 1)
	assert.True(t, rows[0].WithinTolerance, "FP drift 1e-12 must be within_tolerance for MEAN")
}

// TestDiffer_FPDriftExceedsTolerance verifies that floating-point drift larger
// than the tolerance threshold is rejected.
//   - orig = 0.5, cand = 0.5 + 1e-8
//   - diff_rel = 1e-8 / max(0.5, 1) = 1e-8 >> 1e-9
func TestDiffer_FPDriftExceedsTolerance(t *testing.T) {
	ms := NewMockStore()
	reader := NewMockValueReader()
	run := newRunForTest(origMetric)

	const orig = 0.5
	const drift = 1e-8
	reader.SetValues(origMetric, testExpID, testDate, map[string]float64{"treatment": orig})
	reader.SetValues(run.ShadowID.String(), testExpID, testDate, map[string]float64{"treatment": orig + drift})

	d := NewDiffer(reader, ms)
	require.NoError(t, d.Run(context.Background(), run, testExpID, testDate, "MEAN"))

	rows := resultRowsForShadow(t, ms, run.ShadowID)
	require.Len(t, rows, 1)
	assert.False(t, rows[0].WithinTolerance, "FP drift 1e-8 must NOT be within_tolerance for MEAN")
}

// TestDiffer_CountDriftBy1_NotWithinTolerance verifies that any non-zero
// diff_abs for COUNT metrics is rejected, even a difference of 1.
func TestDiffer_CountDriftBy1_NotWithinTolerance(t *testing.T) {
	ms := NewMockStore()
	reader := NewMockValueReader()
	run := newRunForTest(origMetric)

	reader.SetValues(origMetric, testExpID, testDate, map[string]float64{"treatment": 100})
	reader.SetValues(run.ShadowID.String(), testExpID, testDate, map[string]float64{"treatment": 101})

	d := NewDiffer(reader, ms)
	require.NoError(t, d.Run(context.Background(), run, testExpID, testDate, "COUNT"))

	rows := resultRowsForShadow(t, ms, run.ShadowID)
	require.Len(t, rows, 1)
	assert.False(t, rows[0].WithinTolerance, "COUNT diff_abs=1 must NOT be within_tolerance")
	assert.True(t, rows[0].DiffAbs.Valid)
	assert.Equal(t, 1.0, rows[0].DiffAbs.Float64)
}

// TestDiffer_ProportionDriftAnyAmount_NotWithinTolerance verifies that any
// non-zero difference for PROPORTION metrics is rejected — PROPORTION requires
// exact match just like COUNT.
func TestDiffer_ProportionDriftAnyAmount_NotWithinTolerance(t *testing.T) {
	ms := NewMockStore()
	reader := NewMockValueReader()
	run := newRunForTest(origMetric)

	reader.SetValues(origMetric, testExpID, testDate, map[string]float64{"treatment": 0.5})
	reader.SetValues(run.ShadowID.String(), testExpID, testDate, map[string]float64{"treatment": 0.50000001})

	d := NewDiffer(reader, ms)
	require.NoError(t, d.Run(context.Background(), run, testExpID, testDate, "PROPORTION"))

	rows := resultRowsForShadow(t, ms, run.ShadowID)
	require.Len(t, rows, 1)
	assert.False(t, rows[0].WithinTolerance, "PROPORTION with any non-zero diff_abs must NOT be within_tolerance")
}

// TestDiffer_OneSideMissing_OriginalOnly verifies one-side-missing handling
// when the original has a variant absent from the candidate.
//   - orig  = {treatment: 0.5, control: 0.6}
//   - cand  = {treatment: 0.5}
//   - expected: treatment row within_tolerance=true; control row with candidate_value=NULL
func TestDiffer_OneSideMissing_OriginalOnly(t *testing.T) {
	ms := NewMockStore()
	reader := NewMockValueReader()
	run := newRunForTest(origMetric)

	reader.SetValues(origMetric, testExpID, testDate, map[string]float64{"treatment": 0.5, "control": 0.6})
	reader.SetValues(run.ShadowID.String(), testExpID, testDate, map[string]float64{"treatment": 0.5})

	d := NewDiffer(reader, ms)
	require.NoError(t, d.Run(context.Background(), run, testExpID, testDate, "MEAN"))

	rows := resultRowsForShadow(t, ms, run.ShadowID)
	require.Len(t, rows, 2)

	byVariant := make(map[string]ResultRow, 2)
	for _, r := range rows {
		byVariant[r.VariantID] = r
	}

	// treatment: both sides present → within_tolerance = true
	treatment := byVariant["treatment"]
	assert.True(t, treatment.OriginalValue.Valid)
	assert.True(t, treatment.CandidateValue.Valid)
	assert.True(t, treatment.WithinTolerance, "treatment row must be within_tolerance")

	// control: only original present → candidate_value NULL, within_tolerance = false
	control := byVariant["control"]
	assert.True(t, control.OriginalValue.Valid, "control OriginalValue must be set")
	assert.Equal(t, 0.6, control.OriginalValue.Float64)
	assert.False(t, control.CandidateValue.Valid, "control CandidateValue must be NULL (missing on candidate)")
	assert.False(t, control.DiffAbs.Valid, "control DiffAbs must be NULL when one side is missing")
	assert.False(t, control.DiffRel.Valid, "control DiffRel must be NULL when one side is missing")
	assert.False(t, control.WithinTolerance, "control row with missing candidate must NOT be within_tolerance")
}

// TestDiffer_OneSideMissing_CandidateOnly verifies one-side-missing handling
// when the candidate has a variant absent from the original (symmetric case).
func TestDiffer_OneSideMissing_CandidateOnly(t *testing.T) {
	ms := NewMockStore()
	reader := NewMockValueReader()
	run := newRunForTest(origMetric)

	reader.SetValues(origMetric, testExpID, testDate, map[string]float64{"treatment": 0.5})
	reader.SetValues(run.ShadowID.String(), testExpID, testDate, map[string]float64{"treatment": 0.5, "extra_variant": 0.3})

	d := NewDiffer(reader, ms)
	require.NoError(t, d.Run(context.Background(), run, testExpID, testDate, "MEAN"))

	rows := resultRowsForShadow(t, ms, run.ShadowID)
	require.Len(t, rows, 2)

	byVariant := make(map[string]ResultRow, 2)
	for _, r := range rows {
		byVariant[r.VariantID] = r
	}

	// treatment: both sides present → within_tolerance = true
	assert.True(t, byVariant["treatment"].WithinTolerance)

	// extra_variant: only candidate present → original_value NULL, within_tolerance = false
	extra := byVariant["extra_variant"]
	assert.False(t, extra.OriginalValue.Valid, "extra_variant OriginalValue must be NULL (missing on original)")
	assert.True(t, extra.CandidateValue.Valid)
	assert.Equal(t, 0.3, extra.CandidateValue.Float64)
	assert.False(t, extra.DiffAbs.Valid)
	assert.False(t, extra.DiffRel.Valid)
	assert.False(t, extra.WithinTolerance)
}

// TestDiffer_OriginalIsZero_NoDivByZero verifies that orig == 0 uses
// max(0, 1) = 1 as the denominator, preventing division-by-zero.
//   - orig = 0.0, cand = 1e-12
//   - denom = max(0, 1) = 1 → diff_rel = 1e-12 → within_tolerance = true
func TestDiffer_OriginalIsZero_NoDivByZero(t *testing.T) {
	ms := NewMockStore()
	reader := NewMockValueReader()
	run := newRunForTest(origMetric)

	reader.SetValues(origMetric, testExpID, testDate, map[string]float64{"treatment": 0.0})
	reader.SetValues(run.ShadowID.String(), testExpID, testDate, map[string]float64{"treatment": 1e-12})

	d := NewDiffer(reader, ms)
	require.NoError(t, d.Run(context.Background(), run, testExpID, testDate, "MEAN"))

	rows := resultRowsForShadow(t, ms, run.ShadowID)
	require.Len(t, rows, 1)
	assert.True(t, rows[0].WithinTolerance, "zero orig with 1e-12 cand must be within_tolerance (denom=1)")
	assert.True(t, rows[0].DiffAbs.Valid)
	assert.InDelta(t, 1e-12, rows[0].DiffAbs.Float64, 1e-25)
	assert.True(t, rows[0].DiffRel.Valid)
	assert.InDelta(t, 1e-12, rows[0].DiffRel.Float64, 1e-25)
}

// TestDiffer_PersistsToStore verifies that Run actually calls InsertResult for
// each per-variant row (not just in-memory computation).
func TestDiffer_PersistsToStore(t *testing.T) {
	ms := NewMockStore()
	reader := NewMockValueReader()
	run := newRunForTest(origMetric)

	reader.SetValues(origMetric, testExpID, testDate, map[string]float64{
		"control":   1.0,
		"treatment": 1.5,
	})
	reader.SetValues(run.ShadowID.String(), testExpID, testDate, map[string]float64{
		"control":   1.0,
		"treatment": 1.5,
	})

	d := NewDiffer(reader, ms)
	require.NoError(t, d.Run(context.Background(), run, testExpID, testDate, "MEAN"))

	// Query the mock store directly — results must be there.
	stored, err := ms.Results(context.Background(), run.ShadowID)
	require.NoError(t, err)

	// Filter stubs (shouldn't be any, but defensive).
	var realRows []ResultRow
	for _, r := range stored {
		if r.VariantID != "" {
			realRows = append(realRows, r)
		}
	}
	assert.Len(t, realRows, 2, "Differ.Run must have persisted 2 per-variant rows via InsertResult")

	// Every row must have a non-nil ResultID (assigned by MockStore.InsertResult).
	for _, r := range realRows {
		assert.NotEqual(t, uuid.Nil, r.ResultID, "every persisted row must have a ResultID")
	}
}

// TestDiffer_ReadError_Propagated verifies that when ValueReader.Read returns
// an error, Differ.Run returns that error without writing any rows.
func TestDiffer_ReadError_Propagated(t *testing.T) {
	ms := NewMockStore()
	sentinel := fmt.Errorf("spark read failure")
	reader := &errValueReader{err: sentinel}
	run := newRunForTest(origMetric)

	d := NewDiffer(reader, ms)
	err := d.Run(context.Background(), run, testExpID, testDate, "MEAN")
	assert.ErrorIs(t, err, sentinel, "reader error must be propagated")

	// No rows must have been written.
	rows := resultRowsForShadow(t, ms, run.ShadowID)
	assert.Empty(t, rows, "no rows must be written when the reader errors")
}

// TestDiffer_EmptyOutput_NoRows verifies that when both sides return no values
// (e.g. the metric produced no output for this experiment on this date),
// Differ.Run writes zero rows and returns nil.
func TestDiffer_EmptyOutput_NoRows(t *testing.T) {
	ms := NewMockStore()
	reader := NewMockValueReader()
	run := newRunForTest(origMetric)

	// No values seeded → MockValueReader returns empty maps.

	d := NewDiffer(reader, ms)
	require.NoError(t, d.Run(context.Background(), run, testExpID, testDate, "MEAN"))

	rows := resultRowsForShadow(t, ms, run.ShadowID)
	assert.Empty(t, rows, "empty output on both sides must produce zero ResultRows")
}

// ---------------------------------------------------------------------------
// tolerate unit tests (white-box, no I/O)
// ---------------------------------------------------------------------------

func TestTolerate_Count_ExactMatch(t *testing.T) {
	assert.True(t, tolerate(100, 100, "COUNT"), "COUNT identical values must tolerate")
}

func TestTolerate_Count_DiffBy1(t *testing.T) {
	assert.False(t, tolerate(100, 101, "COUNT"), "COUNT diff_abs=1 must not tolerate")
}

func TestTolerate_Proportion_ExactMatch(t *testing.T) {
	assert.True(t, tolerate(0.5, 0.5, "PROPORTION"))
}

func TestTolerate_Proportion_TinyDiff(t *testing.T) {
	assert.False(t, tolerate(0.5, 0.5+1e-15, "PROPORTION"),
		"PROPORTION tiny diff must not tolerate (exact match required)")
}

func TestTolerate_Mean_WithinFPTolerance(t *testing.T) {
	assert.True(t, tolerate(0.5, 0.5+1e-12, "MEAN"))
}

func TestTolerate_Mean_ExceedsFPTolerance(t *testing.T) {
	assert.False(t, tolerate(0.5, 0.5+1e-8, "MEAN"))
}

func TestTolerate_CaseInsensitive(t *testing.T) {
	assert.False(t, tolerate(100, 101, "count"), "lowercase 'count' must be treated as COUNT")
	assert.False(t, tolerate(0.5, 0.50000001, "proportion"), "lowercase 'proportion' must be treated as PROPORTION")
}

func TestTolerate_ZeroOrig_NoDivByZero(t *testing.T) {
	// orig=0 uses denom=max(0,1)=1 → diff_rel = 1e-12 → within tolerance
	assert.True(t, tolerate(0.0, 1e-12, "MEAN"))
	// diff_rel = 2e-9 (just above 1e-9 but wait: 2e-9 > 1e-9)
	assert.False(t, tolerate(0.0, 2e-9, "MEAN"))
}
