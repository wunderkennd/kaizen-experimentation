package jobs

import (
	"context"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"

	"github.com/org/experimentation-platform/services/metrics/internal/alerts"
	"github.com/org/experimentation-platform/services/metrics/internal/config"
	"github.com/org/experimentation-platform/services/metrics/internal/querylog"
	"github.com/org/experimentation-platform/services/metrics/internal/spark"
)

// --- SLA budget constants ---
//
// Production Spark query latency: ~90s average
// Daily SLA: 2h = 7200s -> max 80 queries (7200/90)
// Guardrail SLA: 30min = 1800s -> max 20 queries (1800/90)
const (
	MaxDailyQueries     = 80
	MaxGuardrailQueries = 20
	prodQueryLatencyAvg = 90 * time.Second
)

// --- timedMockExecutor ---

// timedMockExecutor wraps spark.MockExecutor and adds a configurable sleep
// before each call, simulating wall-clock query latency.
type timedMockExecutor struct {
	inner   *spark.MockExecutor
	latency time.Duration
}

func newTimedMockExecutor(rowCount int64, latency time.Duration) *timedMockExecutor {
	return &timedMockExecutor{
		inner:   spark.NewMockExecutor(rowCount),
		latency: latency,
	}
}

func (t *timedMockExecutor) ExecuteSQL(ctx context.Context, sql string) (*spark.SQLResult, error) {
	time.Sleep(t.latency)
	return t.inner.ExecuteSQL(ctx, sql)
}

func (t *timedMockExecutor) ExecuteAndWrite(ctx context.Context, sql string, targetTable string) (*spark.SQLResult, error) {
	time.Sleep(t.latency)
	return t.inner.ExecuteAndWrite(ctx, sql, targetTable)
}

func (t *timedMockExecutor) GetCalls() []spark.MockCall {
	return t.inner.GetCalls()
}

func (t *timedMockExecutor) Reset() {
	t.inner.Reset()
}

// --- Helpers ---

func loadSLAConfig(t *testing.T) *config.ConfigStore {
	t.Helper()
	cfg, err := config.LoadFromFile("../config/testdata/seed_config.json")
	require.NoError(t, err)
	return cfg
}

func newSLARenderer(t *testing.T) *spark.SQLRenderer {
	t.Helper()
	r, err := spark.NewSQLRenderer()
	require.NoError(t, err)
	return r
}

// runDailyPipeline runs StandardJob + ContentConsumptionJob + InterleavingJob
// for all running experiments. Returns total executor call count.
func runDailyPipeline(t *testing.T, cfg *config.ConfigStore, executor spark.SQLExecutor, qlWriter querylog.Writer) int {
	t.Helper()
	ctx := context.Background()
	renderer := newSLARenderer(t)

	stdJob := NewStandardJob(cfg, renderer, executor, qlWriter)
	ccJob := NewContentConsumptionJob(cfg, renderer, executor, qlWriter)
	ilJob := NewInterleavingJob(cfg, renderer, executor, qlWriter)

	for _, expID := range cfg.RunningExperimentIDs() {
		_, err := stdJob.Run(ctx, expID)
		require.NoError(t, err, "StandardJob failed for %s", expID)

		_, err = ccJob.Run(ctx, expID)
		require.NoError(t, err, "ContentConsumptionJob failed for %s", expID)

		_, err = ilJob.Run(ctx, expID)
		require.NoError(t, err, "InterleavingJob failed for %s", expID)
	}

	return len(qlWriter.(*querylog.MemWriter).AllEntries())
}

// runGuardrailPipeline runs GuardrailJob for all running experiments.
// Returns total query log entry count.
func runGuardrailPipeline(t *testing.T, cfg *config.ConfigStore, executor spark.SQLExecutor, qlWriter *querylog.MemWriter) int {
	t.Helper()
	ctx := context.Background()
	renderer := newSLARenderer(t)
	publisher := alerts.NewMemPublisher()
	tracker := alerts.NewBreachTracker()

	// Seed mock values for e001's guardrails so GetVariantValues doesn't fail.
	vp := NewMockValueProvider()
	cv := "f0000000-0000-0000-0000-000000000001"
	tv := "f0000000-0000-0000-0000-000000000002"
	vp.SetVariantValue("rebuffer_rate", cv, 0.02)
	vp.SetVariantValue("rebuffer_rate", tv, 0.03)
	vp.SetVariantValue("error_rate", cv, 0.005)
	vp.SetVariantValue("error_rate", tv, 0.008)

	job := NewGuardrailJob(cfg, renderer, executor, qlWriter, publisher, tracker, vp)

	for _, expID := range cfg.RunningExperimentIDs() {
		_, err := job.Run(ctx, expID)
		require.NoError(t, err, "GuardrailJob failed for %s", expID)
	}

	return len(qlWriter.AllEntries())
}

// countJobTypes tallies query log entries by JobType.
func countJobTypes(entries []querylog.Entry) map[string]int {
	counts := make(map[string]int)
	for _, e := range entries {
		counts[e.JobType]++
	}
	return counts
}

// generateScaledConfig builds a config with n simple AB experiments, writes
// it to a temp file, and loads via LoadFromFile.
func generateScaledConfig(t *testing.T, n int) *config.ConfigStore {
	t.Helper()

	type variant struct {
		VariantID       string  `json:"variant_id"`
		Name            string  `json:"name"`
		TrafficFraction float64 `json:"traffic_fraction"`
		IsControl       bool    `json:"is_control"`
	}
	type experiment struct {
		ExperimentID     string   `json:"experiment_id"`
		Name             string   `json:"name"`
		Type             string   `json:"type"`
		State            string   `json:"state"`
		StartedAt        string   `json:"started_at"`
		PrimaryMetricID  string   `json:"primary_metric_id"`
		SecondaryMetricIDs []string `json:"secondary_metric_ids"`
		Variants         []variant `json:"variants"`
	}
	type metric struct {
		MetricID        string `json:"metric_id"`
		Name            string `json:"name"`
		Type            string `json:"type"`
		SourceEventType string `json:"source_event_type"`
	}
	type seedFile struct {
		Experiments []experiment `json:"experiments"`
		Metrics     []metric     `json:"metrics"`
	}

	sf := seedFile{}

	for i := 0; i < n; i++ {
		eid := fmt.Sprintf("scaled-%04d", i)
		mid := fmt.Sprintf("metric-%04d", i)

		sf.Metrics = append(sf.Metrics, metric{
			MetricID:        mid,
			Name:            fmt.Sprintf("Metric %d", i),
			Type:            "MEAN",
			SourceEventType: "heartbeat",
		})

		sf.Experiments = append(sf.Experiments, experiment{
			ExperimentID:     eid,
			Name:             fmt.Sprintf("experiment_%d", i),
			Type:             "AB",
			State:            "RUNNING",
			StartedAt:        "2024-01-01",
			PrimaryMetricID:  mid,
			SecondaryMetricIDs: nil,
			Variants: []variant{
				{VariantID: fmt.Sprintf("cv-%04d", i), Name: "control", TrafficFraction: 0.5, IsControl: true},
				{VariantID: fmt.Sprintf("tv-%04d", i), Name: "treatment", TrafficFraction: 0.5, IsControl: false},
			},
		})
	}

	data, err := json.MarshalIndent(sf, "", "  ")
	require.NoError(t, err)

	dir := t.TempDir()
	path := filepath.Join(dir, "scaled_config.json")
	require.NoError(t, os.WriteFile(path, data, 0o644))

	cfg, err := config.LoadFromFile(path)
	require.NoError(t, err)
	return cfg
}

// =================================================================
// Test 1: Daily pipeline total query budget
// =================================================================

func TestSLA_DailyPipeline_QueryBudget(t *testing.T) {
	cfg := loadSLAConfig(t)
	executor := spark.NewMockExecutor(500)
	qlWriter := querylog.NewMemWriter()

	total := runDailyPipeline(t, cfg, executor, qlWriter)

	// Exact regression check: 35 standard + 6 content_consumption + 1 interleaving
	// + 1 mlrate_metric + 4 mlrate (1 feat + 3 crossfit) + 1 mlrate_treatment_effect + 1 mlrate_cc = 49.
	assert.Equal(t, 49, total,
		"Daily pipeline query count regression: expected exactly 49 SQL queries")

	// SLA budget check.
	assert.Less(t, total, MaxDailyQueries,
		"Daily pipeline must stay under %d-query SLA budget (got %d)", MaxDailyQueries, total)
}

// =================================================================
// Test 2: Per-experiment StandardJob query breakdown
// =================================================================

func TestSLA_DailyPipeline_PerExperimentBreakdown(t *testing.T) {
	tests := []struct {
		name         string
		experimentID string
		wantTotal    int
		wantJobTypes map[string]int
	}{
		{
			name:         "e001_homepage_recs_v2_4metrics_2cuped_1ratio",
			experimentID: "e0000000-0000-0000-0000-000000000001",
			wantTotal:    11,
			wantJobTypes: map[string]int{
				"daily_metric":           4, // ctr_recommendation + watch_time_minutes + stream_start_rate + rebuffer_rate
				"cuped_covariate":        2, // watch_time_minutes + ctr_recommendation
				"delta_method":           1, // rebuffer_rate (RATIO)
				"daily_treatment_effect": 4, // 4 metrics
			},
		},
		{
			name:         "e003_search_interleave_2metrics_1cuped",
			experimentID: "e0000000-0000-0000-0000-000000000003",
			wantTotal:    5,
			wantJobTypes: map[string]int{
				"daily_metric":           2, // search_success_rate + ctr_recommendation
				"cuped_covariate":        1, // ctr_recommendation
				"daily_treatment_effect": 2,
			},
		},
		{
			name:         "e005_custom_metric_2metrics_1cuped",
			experimentID: "e0000000-0000-0000-0000-000000000005",
			wantTotal:    5,
			wantJobTypes: map[string]int{
				"daily_metric":           2, // watch_time_minutes + power_users_watch_time
				"cuped_covariate":        1, // watch_time_minutes
				"daily_treatment_effect": 2,
			},
		},
		{
			name:         "e006_percentile_1metric",
			experimentID: "e0000000-0000-0000-0000-000000000006",
			wantTotal:    2,
			wantJobTypes: map[string]int{
				"daily_metric":           1, // latency_p50_ms
				"daily_treatment_effect": 1,
			},
		},
		{
			name:         "e007_mixed_qoe_engagement_session_lifecycle",
			experimentID: "e0000000-0000-0000-0000-000000000007",
			wantTotal:    8,
			wantJobTypes: map[string]int{
				"qoe_metric":                 1, // ttff_mean
				"daily_metric":               1, // watch_time_minutes
				"cuped_covariate":            1, // watch_time_minutes
				"session_level_metric":       1, // watch_time_minutes (QoE excluded)
				"lifecycle_metric":           1, // watch_time_minutes (QoE excluded)
				"daily_treatment_effect":     2,
				"qoe_engagement_correlation": 1, // 1 QoE x 1 engagement
			},
		},
		{
			name:         "e004_qoe_only_2metrics",
			experimentID: "e0000000-0000-0000-0000-000000000004",
			wantTotal:    4,
			wantJobTypes: map[string]int{
				"qoe_metric":            2, // ttff_mean + rebuffer_ratio_mean
				"daily_treatment_effect": 2,
			},
		},
	}

	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			cfg := loadSLAConfig(t)
			renderer := newSLARenderer(t)
			executor := spark.NewMockExecutor(500)
			qlWriter := querylog.NewMemWriter()

			job := NewStandardJob(cfg, renderer, executor, qlWriter)
			_, err := job.Run(context.Background(), tc.experimentID)
			require.NoError(t, err)

			entries := qlWriter.AllEntries()
			assert.Equal(t, tc.wantTotal, len(entries),
				"experiment %s: total query count mismatch", tc.experimentID)

			gotJobTypes := countJobTypes(entries)
			for jobType, wantCount := range tc.wantJobTypes {
				if wantCount == 0 {
					continue
				}
				assert.Equal(t, wantCount, gotJobTypes[jobType],
					"experiment %s: job_type=%s count mismatch", tc.experimentID, jobType)
			}
		})
	}
}

// =================================================================
// Test 3: Guardrail pipeline query budget
// =================================================================

func TestSLA_GuardrailPipeline_QueryBudget(t *testing.T) {
	cfg := loadSLAConfig(t)
	executor := spark.NewMockExecutor(500)
	qlWriter := querylog.NewMemWriter()

	total := runGuardrailPipeline(t, cfg, executor, qlWriter)

	// Only e001 has guardrails (2: rebuffer_rate + error_rate).
	assert.Equal(t, 2, total,
		"Guardrail pipeline should produce exactly 2 SQL queries (only e001 has guardrails)")

	assert.Less(t, total, MaxGuardrailQueries,
		"Guardrail pipeline must stay under %d-query SLA budget (got %d)", MaxGuardrailQueries, total)

	// Verify only e001 produced entries.
	for _, e := range qlWriter.AllEntries() {
		assert.Equal(t, "e0000000-0000-0000-0000-000000000001", e.ExperimentID,
			"Only e001 should produce guardrail query log entries")
		assert.Equal(t, "hourly_guardrail", e.JobType)
	}
}

// =================================================================
// Test 4: Daily pipeline wall-clock timing
// =================================================================

func TestSLA_DailyPipeline_WallClock(t *testing.T) {
	cfg := loadSLAConfig(t)
	executor := newTimedMockExecutor(500, 1*time.Millisecond)
	qlWriter := querylog.NewMemWriter()

	start := time.Now()
	_ = runDailyPipeline(t, cfg, executor, qlWriter)
	elapsed := time.Since(start)

	// 42 queries x 1ms = 42ms theoretical. Allow 500ms (>10x headroom)
	// to catch accidental O(n^2) loops without flaking on CI.
	assert.Less(t, elapsed, 500*time.Millisecond,
		"Daily pipeline wall-clock time should be < 500ms with 1ms/query mock (got %v)", elapsed)
}

// =================================================================
// Test 5: Guardrail pipeline wall-clock timing
// =================================================================

func TestSLA_GuardrailPipeline_WallClock(t *testing.T) {
	cfg := loadSLAConfig(t)
	executor := newTimedMockExecutor(500, 1*time.Millisecond)
	qlWriter := querylog.NewMemWriter()

	start := time.Now()
	_ = runGuardrailPipeline(t, cfg, executor, qlWriter)
	elapsed := time.Since(start)

	// 2 queries x 1ms = 2ms theoretical. Allow 100ms headroom.
	assert.Less(t, elapsed, 100*time.Millisecond,
		"Guardrail pipeline wall-clock time should be < 100ms with 1ms/query mock (got %v)", elapsed)
}

// =================================================================
// Test 6: Query count formula per metric type
// =================================================================

func TestSLA_QueryCountFormula_PerMetricType(t *testing.T) {
	tests := []struct {
		name       string
		config     string // JSON config
		wantQueries int
	}{
		{
			name: "MEAN_no_features",
			config: `{
				"experiments": [{
					"experiment_id": "sla-f-001", "name": "t", "type": "AB", "state": "RUNNING",
					"started_at": "2024-01-01", "primary_metric_id": "m1",
					"variants": [
						{"variant_id": "cv", "name": "control", "traffic_fraction": 0.5, "is_control": true},
						{"variant_id": "tv", "name": "treatment", "traffic_fraction": 0.5, "is_control": false}
					]
				}],
				"metrics": [{"metric_id": "m1", "name": "m", "type": "MEAN", "source_event_type": "hb"}]
			}`,
			wantQueries: 2, // base + treatment_effect
		},
		{
			name: "MEAN_with_session",
			config: `{
				"experiments": [{
					"experiment_id": "sla-f-002", "name": "t", "type": "AB", "state": "RUNNING",
					"started_at": "2024-01-01", "primary_metric_id": "m1", "session_level": true,
					"variants": [
						{"variant_id": "cv", "name": "control", "traffic_fraction": 0.5, "is_control": true},
						{"variant_id": "tv", "name": "treatment", "traffic_fraction": 0.5, "is_control": false}
					]
				}],
				"metrics": [{"metric_id": "m1", "name": "m", "type": "MEAN", "source_event_type": "hb"}]
			}`,
			wantQueries: 3, // base + session + treatment_effect
		},
		{
			name: "MEAN_with_lifecycle",
			config: `{
				"experiments": [{
					"experiment_id": "sla-f-003", "name": "t", "type": "AB", "state": "RUNNING",
					"started_at": "2024-01-01", "primary_metric_id": "m1",
					"lifecycle_stratification_enabled": true, "lifecycle_segments": ["TRIAL", "NEW"],
					"variants": [
						{"variant_id": "cv", "name": "control", "traffic_fraction": 0.5, "is_control": true},
						{"variant_id": "tv", "name": "treatment", "traffic_fraction": 0.5, "is_control": false}
					]
				}],
				"metrics": [{"metric_id": "m1", "name": "m", "type": "MEAN", "source_event_type": "hb"}]
			}`,
			wantQueries: 3, // base + lifecycle + treatment_effect
		},
		{
			name: "MEAN_cuped_session_lifecycle",
			config: `{
				"experiments": [{
					"experiment_id": "sla-f-004", "name": "t", "type": "AB", "state": "RUNNING",
					"started_at": "2024-01-01", "primary_metric_id": "m1",
					"session_level": true, "lifecycle_stratification_enabled": true,
					"lifecycle_segments": ["TRIAL"],
					"variants": [
						{"variant_id": "cv", "name": "control", "traffic_fraction": 0.5, "is_control": true},
						{"variant_id": "tv", "name": "treatment", "traffic_fraction": 0.5, "is_control": false}
					]
				}],
				"metrics": [{
					"metric_id": "m1", "name": "m", "type": "MEAN", "source_event_type": "hb",
					"cuped_covariate_metric_id": "m1"
				}]
			}`,
			wantQueries: 5, // base + cuped + session + lifecycle + treatment_effect
		},
		{
			name: "RATIO_no_features",
			config: `{
				"experiments": [{
					"experiment_id": "sla-f-005", "name": "t", "type": "AB", "state": "RUNNING",
					"started_at": "2024-01-01", "primary_metric_id": "m1",
					"variants": [
						{"variant_id": "cv", "name": "control", "traffic_fraction": 0.5, "is_control": true},
						{"variant_id": "tv", "name": "treatment", "traffic_fraction": 0.5, "is_control": false}
					]
				}],
				"metrics": [{
					"metric_id": "m1", "name": "m", "type": "RATIO", "source_event_type": "ev",
					"numerator_event_type": "num", "denominator_event_type": "den"
				}]
			}`,
			wantQueries: 3, // base + delta_method + treatment_effect
		},
		{
			name: "QoE_MEAN_with_session_lifecycle",
			config: `{
				"experiments": [{
					"experiment_id": "sla-f-006", "name": "t", "type": "AB", "state": "RUNNING",
					"started_at": "2024-01-01", "primary_metric_id": "m1",
					"session_level": true, "lifecycle_stratification_enabled": true,
					"lifecycle_segments": ["TRIAL"],
					"variants": [
						{"variant_id": "cv", "name": "control", "traffic_fraction": 0.5, "is_control": true},
						{"variant_id": "tv", "name": "treatment", "traffic_fraction": 0.5, "is_control": false}
					]
				}],
				"metrics": [{
					"metric_id": "m1", "name": "m", "type": "MEAN", "source_event_type": "qoe",
					"is_qoe_metric": true, "qoe_field": "time_to_first_frame_ms"
				}]
			}`,
			wantQueries: 2, // qoe_metric + treatment_effect (QoE excluded from session/lifecycle)
		},
	}

	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			dir := t.TempDir()
			path := filepath.Join(dir, "config.json")
			require.NoError(t, os.WriteFile(path, []byte(tc.config), 0o644))

			cfg, err := config.LoadFromFile(path)
			require.NoError(t, err)

			renderer := newSLARenderer(t)
			executor := spark.NewMockExecutor(500)
			qlWriter := querylog.NewMemWriter()

			job := NewStandardJob(cfg, renderer, executor, qlWriter)
			expIDs := cfg.RunningExperimentIDs()
			require.Len(t, expIDs, 1)

			_, err = job.Run(context.Background(), expIDs[0])
			require.NoError(t, err)

			entries := qlWriter.AllEntries()
			assert.Equal(t, tc.wantQueries, len(entries),
				"query count mismatch for %s", tc.name)
		})
	}
}

// =================================================================
// Test 7: Scaled config linear query growth
// =================================================================

func TestSLA_ScaledConfig_LinearQueryGrowth(t *testing.T) {
	// Each simple MEAN AB experiment with control produces exactly 2 queries
	// from StandardJob (1 daily_metric + 1 treatment_effect) plus 1 content_consumption.
	// Total per experiment = 3 from daily pipeline (no interleaving for AB).

	cfg6 := loadSLAConfig(t)
	exec6 := spark.NewMockExecutor(500)
	ql6 := querylog.NewMemWriter()
	total6 := runDailyPipeline(t, cfg6, exec6, ql6)
	numExps6 := len(cfg6.RunningExperimentIDs())
	perExp6 := float64(total6) / float64(numExps6)

	cfg20 := generateScaledConfig(t, 20)
	exec20 := spark.NewMockExecutor(500)
	ql20 := querylog.NewMemWriter()
	total20 := runDailyPipeline(t, cfg20, exec20, ql20)
	numExps20 := len(cfg20.RunningExperimentIDs())
	perExp20 := float64(total20) / float64(numExps20)

	// Per-experiment ratio should be within 30% tolerance, proving linear scaling.
	ratio := perExp20 / perExp6
	assert.Greater(t, ratio, 0.3,
		"Scaled config should scale linearly: perExp20=%.1f perExp6=%.1f ratio=%.2f",
		perExp20, perExp6, ratio)
	assert.Less(t, ratio, 1.7,
		"Scaled config should scale linearly: perExp20=%.1f perExp6=%.1f ratio=%.2f",
		perExp20, perExp6, ratio)

	// Also verify 20-experiment budget stays under SLA.
	assert.Less(t, total20, MaxDailyQueries,
		"20-experiment daily pipeline should stay under SLA budget (got %d)", total20)

	t.Logf("seed config: %d experiments, %d queries (%.1f/exp)", numExps6, total6, perExp6)
	t.Logf("scaled config: %d experiments, %d queries (%.1f/exp)", numExps20, total20, perExp20)
}
