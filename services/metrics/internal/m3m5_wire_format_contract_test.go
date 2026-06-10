// Package metrics_test contains wire-format contract tests between M3 (Metrics Service)
// and M5 (Management Service). M3 currently loads config from JSON files, but these tests
// validate that M5's ConnectRPC/protojson wire format can be correctly mapped to M3's
// config types. This ensures a future migration from file-based config to live M5 RPC
// calls will not silently break.
//
// The tests follow the "contract snapshot" pattern from m3m5_schema_test.go — define
// mirror types representing M5's wire format and test structural alignment.
package metrics_test

import (
	"encoding/json"
	"reflect"
	"sort"
	"strings"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"

	"github.com/org/experimentation-platform/services/metrics/internal/config"
	commonv1 "github.com/org/experimentation/gen/go/experimentation/common/v1"
)

// ---------------------------------------------------------------------------
// M5 wire-format snapshot types (camelCase protojson)
// ---------------------------------------------------------------------------

// m5WireExperiment mirrors the protojson wire format of common.v1.Experiment
// as returned by M5's GetExperiment/ListExperiments RPCs.
type m5WireExperiment struct {
	ExperimentID        string            `json:"experimentId"`
	Name                string            `json:"name"`
	Description         string            `json:"description,omitempty"`
	OwnerEmail          string            `json:"ownerEmail,omitempty"`
	Type                string            `json:"type"`  // "EXPERIMENT_TYPE_AB"
	State               string            `json:"state"` // "EXPERIMENT_STATE_RUNNING"
	Variants            []m5WireVariant   `json:"variants"`
	LayerID             string            `json:"layerId,omitempty"`
	PrimaryMetricID     string            `json:"primaryMetricId"`
	SecondaryMetricIDs  []string          `json:"secondaryMetricIds,omitempty"`
	GuardrailConfigs    []m5WireGuardrail `json:"guardrailConfigs,omitempty"`
	GuardrailAction     string            `json:"guardrailAction,omitempty"` // "GUARDRAIL_ACTION_AUTO_PAUSE"
	HashSalt            string            `json:"hashSalt,omitempty"`
	StartedAt           string            `json:"startedAt,omitempty"` // RFC3339Nano
	CreatedAt           string            `json:"createdAt,omitempty"`
	ConcludedAt         string            `json:"concludedAt,omitempty"`
	SurrogateModelID    string            `json:"surrogateModelId,omitempty"`
	IsCumulativeHoldout bool              `json:"isCumulativeHoldout,omitempty"` // proto3 zero omit
	InterleavingConfig  *m5WireInterleave `json:"interleavingConfig,omitempty"`
	SessionConfig       *m5WireSession    `json:"sessionConfig,omitempty"`
	LifecycleConfig     *m5WireLifecycle  `json:"lifecycleConfig,omitempty"`
}

type m5WireVariant struct {
	VariantID       string  `json:"variantId"`
	Name            string  `json:"name"`
	TrafficFraction float64 `json:"trafficFraction"`
	IsControl       bool    `json:"isControl,omitempty"` // proto3: false omitted
	PayloadJSON     string  `json:"payloadJson,omitempty"`
}

type m5WireGuardrail struct {
	MetricID                    string  `json:"metricId"`
	Threshold                   float64 `json:"threshold"`
	ConsecutiveBreachesRequired int32   `json:"consecutiveBreachesRequired"`
}

type m5WireInterleave struct {
	CreditAssignment    string `json:"creditAssignment,omitempty"`
	EngagementEventType string `json:"engagementEventType,omitempty"`
}

type m5WireSession struct {
	SessionLevel bool `json:"sessionLevel,omitempty"`
}

type m5WireLifecycle struct {
	Enabled  bool     `json:"enabled,omitempty"`
	Segments []string `json:"segments,omitempty"`
}

// m5WireMetricDefinition mirrors protojson of common.v1.MetricDefinition.
//
// ADR-026 Phase 1 adds three optional oneof type_config arms — FilteredMean,
// Composite, WindowedCount — which protojson emits at the top level (rather
// than nested under `typeConfig`) because the oneof itself isn't named.
type m5WireMetricDefinition struct {
	MetricID                string  `json:"metricId"`
	Name                    string  `json:"name"`
	Description             string  `json:"description,omitempty"`
	Type                    string  `json:"type"` // "METRIC_TYPE_MEAN"
	SourceEventType         string  `json:"sourceEventType,omitempty"`
	NumeratorEventType      string  `json:"numeratorEventType,omitempty"`
	DenominatorEventType    string  `json:"denominatorEventType,omitempty"`
	Percentile              float64 `json:"percentile,omitempty"`
	CustomSQL               string  `json:"customSql,omitempty"`
	LowerIsBetter           bool    `json:"lowerIsBetter,omitempty"`
	SurrogateTargetMetricID string  `json:"surrogateTargetMetricId,omitempty"`
	IsQoEMetric             bool    `json:"isQoeMetric,omitempty"`
	CupedCovariateMetricID  string  `json:"cupedCovariateMetricId,omitempty"`
	MinimumDetectableEffect float64 `json:"minimumDetectableEffect,omitempty"`

	// ADR-026 Phase 1 — oneof type_config arms. Exactly one of these is
	// non-nil when a Phase 1 metric type is used; legacy 6 types leave all nil.
	FilteredMean  *m5WireFilteredMeanConfig  `json:"filteredMean,omitempty"`
	Composite     *m5WireCompositeConfig     `json:"composite,omitempty"`
	WindowedCount *m5WireWindowedCountConfig `json:"windowedCount,omitempty"`
}

// m5WireFilteredMeanConfig mirrors protojson of common.v1.FilteredMeanConfig.
// Populated when MetricDefinition.type == METRIC_TYPE_FILTERED_MEAN.
type m5WireFilteredMeanConfig struct {
	FilterSQL   string `json:"filterSql,omitempty"`
	ValueColumn string `json:"valueColumn,omitempty"`
}

// m5WireCompositeConfig mirrors protojson of common.v1.CompositeConfig.
// Populated when MetricDefinition.type == METRIC_TYPE_COMPOSITE.
type m5WireCompositeConfig struct {
	Operands []m5WireCompositeOperand `json:"operands,omitempty"`
	Operator string                   `json:"operator,omitempty"` // e.g., "COMPOSITE_OPERATOR_WEIGHTED_SUM"
}

// m5WireCompositeOperand mirrors protojson of common.v1.CompositeOperand.
type m5WireCompositeOperand struct {
	MetricID string  `json:"metricId"`
	Weight   float64 `json:"weight,omitempty"` // required > 0 for WEIGHTED_SUM
}

// m5WireWindowedCountConfig mirrors protojson of common.v1.WindowedCountConfig.
// Populated when MetricDefinition.type == METRIC_TYPE_WINDOWED_COUNT.
type m5WireWindowedCountConfig struct {
	EventType   string `json:"eventType,omitempty"`
	FilterSQL   string `json:"filterSql,omitempty"`
	WindowHours int32  `json:"windowHours,omitempty"`
}

// m5CompositeOperatorPrefix is the proto enum prefix for CompositeOperator.
// e.g., "COMPOSITE_OPERATOR_WEIGHTED_SUM" → "WEIGHTED_SUM".
const m5CompositeOperatorPrefix = "COMPOSITE_OPERATOR_"

// m5WireSurrogateModel mirrors protojson of common.v1.SurrogateModelConfig.
type m5WireSurrogateModel struct {
	ModelID               string   `json:"modelId"`
	TargetMetricID        string   `json:"targetMetricId"`
	InputMetricIDs        []string `json:"inputMetricIds,omitempty"`
	ObservationWindowDays int32    `json:"observationWindowDays,omitempty"`
	PredictionHorizonDays int32    `json:"predictionHorizonDays,omitempty"`
	ModelType             string   `json:"modelType"` // "SURROGATE_MODEL_TYPE_LINEAR"
	CalibrationRSquared   float64  `json:"calibrationRSquared,omitempty"`
	MlflowModelURI        string   `json:"mlflowModelUri,omitempty"`
	LastCalibratedAt      string   `json:"lastCalibratedAt,omitempty"`
	CreatedAt             string   `json:"createdAt,omitempty"`
}

// ---------------------------------------------------------------------------
// Enum conversion helpers (what M3 would need for live M5 integration)
// ---------------------------------------------------------------------------

// stripEnumPrefix removes the proto enum prefix from a string.
// e.g., "EXPERIMENT_TYPE_AB" → "AB", "METRIC_TYPE_MEAN" → "MEAN"
func stripEnumPrefix(s, prefix string) string {
	return strings.TrimPrefix(s, prefix)
}

// m5ExperimentTypePrefix is the proto enum prefix for ExperimentType.
const m5ExperimentTypePrefix = "EXPERIMENT_TYPE_"

// m5ExperimentStatePrefix is the proto enum prefix for ExperimentState.
const m5ExperimentStatePrefix = "EXPERIMENT_STATE_"

// m5MetricTypePrefix is the proto enum prefix for MetricType.
const m5MetricTypePrefix = "METRIC_TYPE_"

// m5SurrogateModelTypePrefix is the proto enum prefix for SurrogateModelType.
const m5SurrogateModelTypePrefix = "SURROGATE_MODEL_TYPE_"

// m5GuardrailActionPrefix is the proto enum prefix for GuardrailAction.
const m5GuardrailActionPrefix = "GUARDRAIL_ACTION_"

// ---------------------------------------------------------------------------
// Tests: Experiment field mapping
// ---------------------------------------------------------------------------

// TestM3M5_ExperimentConfig_FieldCompleteness verifies that every field M3's
// ExperimentConfig uses has a corresponding field in M5's protojson wire format.
func TestM3M5_ExperimentConfig_FieldCompleteness(t *testing.T) {
	// M3 config JSON tags → M5 protojson equivalents.
	m3ToM5 := map[string]string{
		"experiment_id":                    "experimentId",
		"name":                             "name",
		"type":                             "type",
		"state":                            "state",
		"started_at":                       "startedAt",
		"primary_metric_id":                "primaryMetricId",
		"secondary_metric_ids":             "secondaryMetricIds",
		"variants":                         "variants",
		"guardrail_configs":                "guardrailConfigs",
		"guardrail_action":                 "guardrailAction",
		"lifecycle_stratification_enabled": "lifecycleConfig.enabled",
		"lifecycle_segments":               "lifecycleConfig.segments",
		"surrogate_model_id":               "surrogateModelId",
		"credit_assignment":                "interleavingConfig.creditAssignment",
		"engagement_event_type":            "interleavingConfig.engagementEventType",
		"session_level":                    "sessionConfig.sessionLevel",
		"mlrate_enabled":                   "mlrateConfig.enabled",
		"mlrate_folds":                     "mlrateConfig.folds",
	}

	// Extract JSON tags from M3's ExperimentConfig struct.
	m3Tags := extractJSONTags(reflect.TypeOf(config.ExperimentConfig{}))

	for m3Tag := range m3ToM5 {
		assert.Contains(t, m3Tags, m3Tag,
			"M3 ExperimentConfig must have JSON tag %q", m3Tag)
	}

	// Every M3 field must have a documented M5 equivalent.
	for _, m3Tag := range m3Tags {
		_, ok := m3ToM5[m3Tag]
		assert.True(t, ok,
			"M3 ExperimentConfig field %q has no documented M5 wire-format equivalent", m3Tag)
	}
}

// TestM3M5_MetricConfig_FieldCompleteness verifies that every field M3's
// MetricConfig uses has a corresponding field in M5's MetricDefinition wire format.
//
// Since #506 M3 embeds *commonv1.MetricDefinition, so the proto-generated JSON
// tags (snake_case, including the oneof TypeConfig arms) appear in extractJSONTags
// alongside the four M3-only sibling fields (qoe_field, mlrate_*).
func TestM3M5_MetricConfig_FieldCompleteness(t *testing.T) {
	m3ToM5 := map[string]string{
		// Proto fields (auto-mirrored via embed):
		"metric_id":                  "metricId",
		"name":                       "name",
		"description":                "description",
		"type":                       "type",
		"source_event_type":          "sourceEventType",
		"numerator_event_type":       "numeratorEventType",
		"denominator_event_type":     "denominatorEventType",
		"percentile":                 "percentile",
		"custom_sql":                 "customSql",
		"lower_is_better":            "lowerIsBetter",
		"surrogate_target_metric_id": "surrogateTargetMetricId",
		"is_qoe_metric":              "isQoeMetric",
		"cuped_covariate_metric_id":  "cupedCovariateMetricId",
		"minimum_detectable_effect":  "minimumDetectableEffect",
		"stakeholder":                "stakeholder",
		"aggregation_level":          "aggregationLevel",
		// ADR-026 Phase 1 — oneof type_config arms (protojson snake_case tags).
		"filtered_mean":  "filteredMean",
		"composite":      "composite",
		"windowed_count": "windowedCount",
		// ADR-026 Phase 2 — METRICQL expression source text.
		"metricql_expression": "metricqlExpression",

		// M3-only sibling fields (not in proto):
		"qoe_field":                  "", // M3-only; not in M5 proto (derived from source_event_type)
		"mlrate_feature_event_types": "mlrateConfig.featureEventTypes",
		"mlrate_model_uri":           "mlrateConfig.modelUri",
		"mlrate_lookback_days":       "mlrateConfig.lookbackDays",
	}

	m3Tags := extractJSONTags(reflect.TypeOf(config.MetricConfig{}))

	for _, m3Tag := range m3Tags {
		_, ok := m3ToM5[m3Tag]
		assert.True(t, ok,
			"M3 MetricConfig field %q has no documented M5 wire-format equivalent", m3Tag)
	}
}

// TestM3M5_SurrogateModelConfig_FieldCompleteness verifies M3's SurrogateModelConfig
// fields have M5 proto counterparts.
func TestM3M5_SurrogateModelConfig_FieldCompleteness(t *testing.T) {
	m3ToM5 := map[string]string{
		"model_id":                "modelId",
		"target_metric_id":        "targetMetricId",
		"input_metric_ids":        "inputMetricIds",
		"observation_window_days": "observationWindowDays",
		"prediction_horizon_days": "predictionHorizonDays",
		"model_type":              "modelType",
		"calibration_r_squared":   "calibrationRSquared",
		"mlflow_model_uri":        "mlflowModelUri",
		"coefficients":            "", // M3-only mock field; not in M5 proto
		"intercept":               "", // M3-only mock field; not in M5 proto
	}

	m3Tags := extractJSONTags(reflect.TypeOf(config.SurrogateModelConfig{}))

	for _, m3Tag := range m3Tags {
		_, ok := m3ToM5[m3Tag]
		assert.True(t, ok,
			"M3 SurrogateModelConfig field %q has no documented M5 wire-format equivalent", m3Tag)
	}
}

// ---------------------------------------------------------------------------
// Tests: Enum format conversion
// ---------------------------------------------------------------------------

// TestM3M5_EnumFormat_ExperimentType validates the mapping between M5's prefixed
// enum strings and M3's short enum strings for all experiment types.
func TestM3M5_EnumFormat_ExperimentType(t *testing.T) {
	cases := []struct {
		m5Wire   string
		m3Config string
	}{
		{"EXPERIMENT_TYPE_AB", "AB"},
		{"EXPERIMENT_TYPE_MULTIVARIATE", "MULTIVARIATE"},
		{"EXPERIMENT_TYPE_INTERLEAVING", "INTERLEAVING"},
		{"EXPERIMENT_TYPE_SESSION_LEVEL", "SESSION_LEVEL"},
		{"EXPERIMENT_TYPE_PLAYBACK_QOE", "PLAYBACK_QOE"},
		{"EXPERIMENT_TYPE_MAB", "MAB"},
		{"EXPERIMENT_TYPE_CONTEXTUAL_BANDIT", "CONTEXTUAL_BANDIT"},
		{"EXPERIMENT_TYPE_CUMULATIVE_HOLDOUT", "CUMULATIVE_HOLDOUT"},
	}

	for _, tc := range cases {
		t.Run(tc.m3Config, func(t *testing.T) {
			got := stripEnumPrefix(tc.m5Wire, m5ExperimentTypePrefix)
			assert.Equal(t, tc.m3Config, got,
				"stripping %q prefix from %q must yield %q",
				m5ExperimentTypePrefix, tc.m5Wire, tc.m3Config)
		})
	}
}

// TestM3M5_EnumFormat_ExperimentState validates all experiment state enum conversions.
func TestM3M5_EnumFormat_ExperimentState(t *testing.T) {
	cases := []struct {
		m5Wire   string
		m3Config string
	}{
		{"EXPERIMENT_STATE_DRAFT", "DRAFT"},
		{"EXPERIMENT_STATE_STARTING", "STARTING"},
		{"EXPERIMENT_STATE_RUNNING", "RUNNING"},
		{"EXPERIMENT_STATE_CONCLUDING", "CONCLUDING"},
		{"EXPERIMENT_STATE_CONCLUDED", "CONCLUDED"},
		{"EXPERIMENT_STATE_ARCHIVED", "ARCHIVED"},
	}

	for _, tc := range cases {
		t.Run(tc.m3Config, func(t *testing.T) {
			got := stripEnumPrefix(tc.m5Wire, m5ExperimentStatePrefix)
			assert.Equal(t, tc.m3Config, got)
		})
	}
}

// TestM3M5_EnumFormat_MetricType validates all metric type enum conversions.
func TestM3M5_EnumFormat_MetricType(t *testing.T) {
	cases := []struct {
		m5Wire   string
		m3Config string
	}{
		{"METRIC_TYPE_MEAN", "MEAN"},
		{"METRIC_TYPE_PROPORTION", "PROPORTION"},
		{"METRIC_TYPE_RATIO", "RATIO"},
		{"METRIC_TYPE_COUNT", "COUNT"},
		{"METRIC_TYPE_PERCENTILE", "PERCENTILE"},
		{"METRIC_TYPE_CUSTOM", "CUSTOM"},
	}

	for _, tc := range cases {
		t.Run(tc.m3Config, func(t *testing.T) {
			got := stripEnumPrefix(tc.m5Wire, m5MetricTypePrefix)
			assert.Equal(t, tc.m3Config, got)
		})
	}
}

// TestM3M5_EnumFormat_SurrogateModelType validates surrogate model type enums.
func TestM3M5_EnumFormat_SurrogateModelType(t *testing.T) {
	cases := []struct {
		m5Wire   string
		m3Config string
	}{
		{"SURROGATE_MODEL_TYPE_LINEAR", "LINEAR"},
		{"SURROGATE_MODEL_TYPE_GRADIENT_BOOSTED", "GRADIENT_BOOSTED"},
		{"SURROGATE_MODEL_TYPE_NEURAL", "NEURAL"},
	}

	for _, tc := range cases {
		t.Run(tc.m3Config, func(t *testing.T) {
			got := stripEnumPrefix(tc.m5Wire, m5SurrogateModelTypePrefix)
			assert.Equal(t, tc.m3Config, got)
		})
	}
}

// TestM3M5_EnumFormat_GuardrailAction validates guardrail action enums.
func TestM3M5_EnumFormat_GuardrailAction(t *testing.T) {
	cases := []struct {
		m5Wire   string
		m3Config string
	}{
		{"GUARDRAIL_ACTION_AUTO_PAUSE", "AUTO_PAUSE"},
		{"GUARDRAIL_ACTION_ALERT_ONLY", "ALERT_ONLY"},
	}

	for _, tc := range cases {
		t.Run(tc.m3Config, func(t *testing.T) {
			got := stripEnumPrefix(tc.m5Wire, m5GuardrailActionPrefix)
			assert.Equal(t, tc.m3Config, got)
		})
	}
}

// ---------------------------------------------------------------------------
// Tests: Proto3 zero-value semantics
// ---------------------------------------------------------------------------

// TestM3M5_Proto3ZeroValueOmission validates that proto3 JSON omits default
// values (false, 0, empty string), and M3's config correctly handles this.
func TestM3M5_Proto3ZeroValueOmission(t *testing.T) {
	t.Run("treatment variant omits isControl=false", func(t *testing.T) {
		// M5 protojson: isControl=false is omitted by default.
		wireJSON := `{"variantId":"v1","name":"treatment","trafficFraction":0.5}`
		var v m5WireVariant
		err := json.Unmarshal([]byte(wireJSON), &v)
		require.NoError(t, err)
		assert.Equal(t, false, v.IsControl, "omitted isControl must default to false")
	})

	t.Run("control variant includes isControl=true", func(t *testing.T) {
		wireJSON := `{"variantId":"v2","name":"control","trafficFraction":0.5,"isControl":true}`
		var v m5WireVariant
		err := json.Unmarshal([]byte(wireJSON), &v)
		require.NoError(t, err)
		assert.Equal(t, true, v.IsControl)
	})

	t.Run("metric lower_is_better=false omitted", func(t *testing.T) {
		wireJSON := `{"metricId":"m1","name":"Watch Time","type":"METRIC_TYPE_MEAN","sourceEventType":"heartbeat"}`
		var m m5WireMetricDefinition
		err := json.Unmarshal([]byte(wireJSON), &m)
		require.NoError(t, err)
		assert.Equal(t, false, m.LowerIsBetter, "omitted lowerIsBetter must default to false")
		assert.Equal(t, false, m.IsQoEMetric, "omitted isQoeMetric must default to false")
	})

	t.Run("metric lower_is_better=true present", func(t *testing.T) {
		wireJSON := `{"metricId":"m2","name":"Rebuffer","type":"METRIC_TYPE_RATIO","lowerIsBetter":true}`
		var m m5WireMetricDefinition
		err := json.Unmarshal([]byte(wireJSON), &m)
		require.NoError(t, err)
		assert.Equal(t, true, m.LowerIsBetter)
	})

	t.Run("empty secondaryMetricIds array omitted", func(t *testing.T) {
		wireJSON := `{"experimentId":"e1","name":"test","type":"EXPERIMENT_TYPE_AB","state":"EXPERIMENT_STATE_RUNNING","primaryMetricId":"m1","variants":[]}`
		var e m5WireExperiment
		err := json.Unmarshal([]byte(wireJSON), &e)
		require.NoError(t, err)
		assert.Nil(t, e.SecondaryMetricIDs, "omitted array must be nil (M3 handles nil as empty)")
	})

	t.Run("zero percentile omitted for non-percentile metrics", func(t *testing.T) {
		wireJSON := `{"metricId":"m3","name":"CTR","type":"METRIC_TYPE_PROPORTION","sourceEventType":"click"}`
		var m m5WireMetricDefinition
		err := json.Unmarshal([]byte(wireJSON), &m)
		require.NoError(t, err)
		assert.Equal(t, 0.0, m.Percentile, "non-percentile metric must have zero percentile")
	})

	t.Run("surrogate calibration_r_squared=0 omitted", func(t *testing.T) {
		wireJSON := `{"modelId":"sm1","targetMetricId":"churn","modelType":"SURROGATE_MODEL_TYPE_LINEAR"}`
		var sm m5WireSurrogateModel
		err := json.Unmarshal([]byte(wireJSON), &sm)
		require.NoError(t, err)
		assert.Equal(t, 0.0, sm.CalibrationRSquared,
			"omitted calibrationRSquared must default to 0 (uncalibrated)")
	})
}

// ---------------------------------------------------------------------------
// Tests: JSON roundtrip M5 wire → M3 config
// ---------------------------------------------------------------------------

// TestM3M5_ExperimentWireToConfig_Roundtrip verifies that an M5 wire-format
// Experiment can be converted to M3's ExperimentConfig (with enum stripping
// and field renaming). This documents the exact transformation M3 must apply.
func TestM3M5_ExperimentWireToConfig_Roundtrip(t *testing.T) {
	m5 := m5WireExperiment{
		ExperimentID:       "e0000001",
		Name:               "homepage_recs_v2",
		Type:               "EXPERIMENT_TYPE_AB",
		State:              "EXPERIMENT_STATE_RUNNING",
		PrimaryMetricID:    "ctr_recommendation",
		SecondaryMetricIDs: []string{"watch_time_minutes"},
		StartedAt:          "2026-03-01T08:00:00Z",
		SurrogateModelID:   "sm-001",
		GuardrailAction:    "GUARDRAIL_ACTION_AUTO_PAUSE",
		Variants: []m5WireVariant{
			{VariantID: "v1", Name: "control", TrafficFraction: 0.5, IsControl: true},
			{VariantID: "v2", Name: "treatment", TrafficFraction: 0.5},
		},
		GuardrailConfigs: []m5WireGuardrail{
			{MetricID: "rebuffer_rate", Threshold: 0.05, ConsecutiveBreachesRequired: 3},
		},
	}

	// Convert M5 wire → M3 config (simulating the adapter).
	m3 := m5ExperimentToConfig(m5)

	assert.Equal(t, "e0000001", m3.ExperimentID)
	assert.Equal(t, "homepage_recs_v2", m3.Name)
	assert.Equal(t, "AB", m3.Type, "type must be stripped of EXPERIMENT_TYPE_ prefix")
	assert.Equal(t, "RUNNING", m3.State, "state must be stripped of EXPERIMENT_STATE_ prefix")
	assert.Equal(t, "ctr_recommendation", m3.PrimaryMetricID)
	assert.Equal(t, []string{"watch_time_minutes"}, m3.SecondaryMetricIDs)
	assert.Equal(t, "2026-03-01T08:00:00Z", m3.StartedAt)
	assert.Equal(t, "sm-001", m3.SurrogateModelID)
	assert.Equal(t, "AUTO_PAUSE", m3.GuardrailAction)

	require.Len(t, m3.Variants, 2)
	assert.Equal(t, "v1", m3.Variants[0].VariantID)
	assert.Equal(t, true, m3.Variants[0].IsControl)
	assert.Equal(t, "v2", m3.Variants[1].VariantID)
	assert.Equal(t, false, m3.Variants[1].IsControl)

	require.Len(t, m3.GuardrailConfigs, 1)
	assert.Equal(t, "rebuffer_rate", m3.GuardrailConfigs[0].MetricID)
	assert.InDelta(t, 0.05, m3.GuardrailConfigs[0].Threshold, 1e-9)
	assert.Equal(t, 3, m3.GuardrailConfigs[0].ConsecutiveBreachesRequired)
}

// TestM3M5_MetricWireToConfig_AllTypes verifies conversion for all 6 metric types.
func TestM3M5_MetricWireToConfig_AllTypes(t *testing.T) {
	cases := []struct {
		name     string
		wire     m5WireMetricDefinition
		wantType commonv1.MetricType
	}{
		{
			name: "MEAN",
			wire: m5WireMetricDefinition{
				MetricID: "watch_time", Name: "Watch Time", Type: "METRIC_TYPE_MEAN",
				SourceEventType: "heartbeat", CupedCovariateMetricID: "watch_time",
			},
			wantType: commonv1.MetricType_METRIC_TYPE_MEAN,
		},
		{
			name: "PROPORTION",
			wire: m5WireMetricDefinition{
				MetricID: "ctr", Name: "CTR", Type: "METRIC_TYPE_PROPORTION",
				SourceEventType: "impression",
			},
			wantType: commonv1.MetricType_METRIC_TYPE_PROPORTION,
		},
		{
			name: "RATIO",
			wire: m5WireMetricDefinition{
				MetricID: "rebuffer_rate", Name: "Rebuffer Rate", Type: "METRIC_TYPE_RATIO",
				NumeratorEventType: "rebuffer_event", DenominatorEventType: "playback_minute",
				LowerIsBetter: true,
			},
			wantType: commonv1.MetricType_METRIC_TYPE_RATIO,
		},
		{
			name: "COUNT",
			wire: m5WireMetricDefinition{
				MetricID: "sessions", Name: "Sessions", Type: "METRIC_TYPE_COUNT",
				SourceEventType: "session_start",
			},
			wantType: commonv1.MetricType_METRIC_TYPE_COUNT,
		},
		{
			name: "PERCENTILE",
			wire: m5WireMetricDefinition{
				MetricID: "latency_p50", Name: "Latency p50", Type: "METRIC_TYPE_PERCENTILE",
				SourceEventType: "playback_start", Percentile: 0.50, LowerIsBetter: true,
			},
			wantType: commonv1.MetricType_METRIC_TYPE_PERCENTILE,
		},
		{
			name: "CUSTOM",
			wire: m5WireMetricDefinition{
				MetricID: "power_users", Name: "Power Users", Type: "METRIC_TYPE_CUSTOM",
				CustomSQL: "SELECT user_id, AVG(value) FROM events GROUP BY user_id HAVING COUNT(*) >= 10",
			},
			wantType: commonv1.MetricType_METRIC_TYPE_CUSTOM,
		},
	}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			m3 := m5MetricToConfig(tc.wire)
			assert.Equal(t, tc.wantType, m3.Type,
				"metric type must round-trip into the proto enum")
			assert.Equal(t, tc.wire.MetricID, m3.MetricId)
			assert.Equal(t, tc.wire.Name, m3.Name)
			assert.Equal(t, tc.wire.LowerIsBetter, m3.LowerIsBetter)
		})
	}
}

// ---------------------------------------------------------------------------
// Tests: ADR-026 Phase 1 oneof type_config arms — wire-format round-trip
// ---------------------------------------------------------------------------

// TestM3M5_WireFormat_FilteredMean verifies that a FILTERED_MEAN metric
// round-trips through M5's protojson wire format with its FilteredMeanConfig
// payload intact, and that M3's flat-field MetricConfig captures both
// filter_sql and value_column.
func TestM3M5_WireFormat_FilteredMean(t *testing.T) {
	original := m5WireMetricDefinition{
		MetricID: "mobile_long_view_avg_duration_ms",
		Name:     "Mobile Long-View Avg Duration (ms)",
		Type:     "METRIC_TYPE_FILTERED_MEAN",
		// FILTERED_MEAN uses the source_event_type for the scan and
		// AVGs value_column over rows passing filter_sql.
		SourceEventType: "playback_heartbeat",
		LowerIsBetter:   false,
		FilteredMean: &m5WireFilteredMeanConfig{
			FilterSQL:   "platform = 'mobile' AND duration_ms > 5000",
			ValueColumn: "duration_ms",
		},
	}

	// Round-trip via protojson wire bytes (M5 emits → M3 reads).
	wire, err := json.Marshal(original)
	require.NoError(t, err, "marshal to protojson wire format")

	var decoded m5WireMetricDefinition
	require.NoError(t, json.Unmarshal(wire, &decoded), "unmarshal protojson wire format")

	assert.Equal(t, "METRIC_TYPE_FILTERED_MEAN", decoded.Type,
		"type must survive protojson round-trip with full enum prefix")
	require.NotNil(t, decoded.FilteredMean,
		"FilteredMean oneof arm must survive protojson round-trip")
	assert.Equal(t, "platform = 'mobile' AND duration_ms > 5000",
		decoded.FilteredMean.FilterSQL,
		"FilteredMeanConfig.filter_sql must survive round-trip byte-for-byte")
	assert.Equal(t, "duration_ms", decoded.FilteredMean.ValueColumn,
		"FilteredMeanConfig.value_column must survive round-trip byte-for-byte")
	assert.Nil(t, decoded.Composite, "non-active oneof arms must remain nil")
	assert.Nil(t, decoded.WindowedCount, "non-active oneof arms must remain nil")

	// Build M3's proto-direct view and assert the oneof FilteredMean payload survives.
	m3 := m5MetricToConfig(decoded)
	assert.Equal(t, commonv1.MetricType_METRIC_TYPE_FILTERED_MEAN, m3.Type,
		"M3 MetricConfig.Type retains the proto enum (no string stripping)")
	assert.Equal(t, "platform = 'mobile' AND duration_ms > 5000", m3.GetFilteredMean().GetFilterSql(),
		"FilteredMean.filter_sql must be populated from the oneof arm")
	assert.Equal(t, "duration_ms", m3.GetFilteredMean().GetValueColumn(),
		"FilteredMean.value_column must be populated from the oneof arm")
	assert.Equal(t, "playback_heartbeat", m3.SourceEventType,
		"FILTERED_MEAN still needs source_event_type to identify the scan table")
}

// TestM3M5_WireFormat_Composite verifies a COMPOSITE metric (WEIGHTED_SUM
// with two operands) round-trips through protojson with operand metric_ids,
// weights, and the operator enum intact.
func TestM3M5_WireFormat_Composite(t *testing.T) {
	original := m5WireMetricDefinition{
		MetricID: "engagement_quality_score",
		Name:     "Engagement Quality Score",
		Type:     "METRIC_TYPE_COMPOSITE",
		Composite: &m5WireCompositeConfig{
			Operator: "COMPOSITE_OPERATOR_WEIGHTED_SUM",
			Operands: []m5WireCompositeOperand{
				{MetricID: "watch_time_minutes", Weight: 0.7},
				{MetricID: "completion_rate", Weight: 0.3},
			},
		},
	}

	wire, err := json.Marshal(original)
	require.NoError(t, err, "marshal to protojson wire format")

	var decoded m5WireMetricDefinition
	require.NoError(t, json.Unmarshal(wire, &decoded), "unmarshal protojson wire format")

	assert.Equal(t, "METRIC_TYPE_COMPOSITE", decoded.Type)
	require.NotNil(t, decoded.Composite,
		"Composite oneof arm must survive protojson round-trip")
	assert.Equal(t, "COMPOSITE_OPERATOR_WEIGHTED_SUM", decoded.Composite.Operator,
		"CompositeConfig.operator enum must survive round-trip with full prefix")
	require.Len(t, decoded.Composite.Operands, 2,
		"both operands must survive round-trip (no array truncation)")
	assert.Equal(t, "watch_time_minutes", decoded.Composite.Operands[0].MetricID)
	assert.InDelta(t, 0.7, decoded.Composite.Operands[0].Weight, 1e-9,
		"WEIGHTED_SUM operand weights must survive as float64")
	assert.Equal(t, "completion_rate", decoded.Composite.Operands[1].MetricID)
	assert.InDelta(t, 0.3, decoded.Composite.Operands[1].Weight, 1e-9)
	assert.Nil(t, decoded.FilteredMean, "non-active oneof arms must remain nil")
	assert.Nil(t, decoded.WindowedCount, "non-active oneof arms must remain nil")

	// Build M3's proto-direct view.
	m3 := m5MetricToConfig(decoded)
	assert.Equal(t, commonv1.MetricType_METRIC_TYPE_COMPOSITE, m3.Type)
	assert.Equal(t, commonv1.CompositeOperator_COMPOSITE_OPERATOR_WEIGHTED_SUM, m3.GetComposite().GetOperator(),
		"Composite.operator round-trips as proto enum")
	operands := m3.GetComposite().GetOperands()
	require.Len(t, operands, 2,
		"M3 MetricConfig.composite.operands must mirror the wire-format array length")
	assert.Equal(t, "watch_time_minutes", operands[0].GetMetricId())
	assert.InDelta(t, 0.7, operands[0].GetWeight(), 1e-9)
	assert.Equal(t, "completion_rate", operands[1].GetMetricId())
	assert.InDelta(t, 0.3, operands[1].GetWeight(), 1e-9)
}

// TestM3M5_WireFormat_WindowedCount verifies a WINDOWED_COUNT metric
// round-trips through protojson with event_type, filter_sql, and window_hours
// intact, and that M3's flat MetricConfig captures all three.
func TestM3M5_WireFormat_WindowedCount(t *testing.T) {
	original := m5WireMetricDefinition{
		MetricID: "trial_signup_7d_count",
		Name:     "Trial Signups within 7 days",
		Type:     "METRIC_TYPE_WINDOWED_COUNT",
		WindowedCount: &m5WireWindowedCountConfig{
			EventType:   "trial_signup",
			FilterSQL:   "country = 'US'",
			WindowHours: 168, // 7 days
		},
	}

	wire, err := json.Marshal(original)
	require.NoError(t, err, "marshal to protojson wire format")

	var decoded m5WireMetricDefinition
	require.NoError(t, json.Unmarshal(wire, &decoded), "unmarshal protojson wire format")

	assert.Equal(t, "METRIC_TYPE_WINDOWED_COUNT", decoded.Type)
	require.NotNil(t, decoded.WindowedCount,
		"WindowedCount oneof arm must survive protojson round-trip")
	assert.Equal(t, "trial_signup", decoded.WindowedCount.EventType,
		"WindowedCountConfig.event_type must survive round-trip")
	assert.Equal(t, "country = 'US'", decoded.WindowedCount.FilterSQL,
		"WindowedCountConfig.filter_sql must survive round-trip byte-for-byte")
	assert.Equal(t, int32(168), decoded.WindowedCount.WindowHours,
		"WindowedCountConfig.window_hours must survive as int32")
	assert.Nil(t, decoded.FilteredMean, "non-active oneof arms must remain nil")
	assert.Nil(t, decoded.Composite, "non-active oneof arms must remain nil")

	// Build M3's proto-direct view.
	m3 := m5MetricToConfig(decoded)
	assert.Equal(t, commonv1.MetricType_METRIC_TYPE_WINDOWED_COUNT, m3.Type)
	assert.Equal(t, "trial_signup", m3.GetWindowedCount().GetEventType(),
		"WindowedCount.event_type (distinct from source_event_type)")
	assert.Equal(t, "country = 'US'", m3.GetWindowedCount().GetFilterSql(),
		"WindowedCount.filter_sql lives on its own oneof arm")
	assert.Equal(t, int32(168), m3.GetWindowedCount().GetWindowHours(),
		"WindowedCount.window_hours must preserve the int32 from the wire")
}

// TestM3M5_MetricWireToConfig_RatioFields verifies RATIO-specific fields map correctly.
func TestM3M5_MetricWireToConfig_RatioFields(t *testing.T) {
	wire := m5WireMetricDefinition{
		MetricID:             "rebuffer_rate",
		Name:                 "Rebuffer Rate",
		Type:                 "METRIC_TYPE_RATIO",
		NumeratorEventType:   "rebuffer_event",
		DenominatorEventType: "playback_minute",
		LowerIsBetter:        true,
	}

	m3 := m5MetricToConfig(wire)
	assert.Equal(t, "rebuffer_event", m3.NumeratorEventType)
	assert.Equal(t, "playback_minute", m3.DenominatorEventType)
	assert.Empty(t, m3.SourceEventType, "RATIO metric should not have source_event_type")
}

// TestM3M5_MetricWireToConfig_QoEFields verifies QoE-specific fields map correctly.
func TestM3M5_MetricWireToConfig_QoEFields(t *testing.T) {
	wire := m5WireMetricDefinition{
		MetricID:        "ttff_mean",
		Name:            "Time to First Frame",
		Type:            "METRIC_TYPE_MEAN",
		SourceEventType: "qoe_playback",
		IsQoEMetric:     true,
		LowerIsBetter:   true,
	}

	m3 := m5MetricToConfig(wire)
	assert.Equal(t, true, m3.IsQoeMetric)
	assert.Equal(t, true, m3.LowerIsBetter)
	assert.Equal(t, "qoe_playback", m3.SourceEventType)
}

// TestM3M5_MetricWireToConfig_CupedCovariate verifies CUPED covariate field mapping.
func TestM3M5_MetricWireToConfig_CupedCovariate(t *testing.T) {
	wire := m5WireMetricDefinition{
		MetricID:               "watch_time",
		Name:                   "Watch Time",
		Type:                   "METRIC_TYPE_MEAN",
		SourceEventType:        "heartbeat",
		CupedCovariateMetricID: "watch_time",
	}

	m3 := m5MetricToConfig(wire)
	assert.Equal(t, "watch_time", m3.CupedCovariateMetricId,
		"CUPED covariate metric ID must be preserved")
}

// TestM3M5_SurrogateWireToConfig_Roundtrip verifies surrogate model conversion.
func TestM3M5_SurrogateWireToConfig_Roundtrip(t *testing.T) {
	wire := m5WireSurrogateModel{
		ModelID:               "sm-churn-001",
		TargetMetricID:        "churn_7d",
		InputMetricIDs:        []string{"watch_time_minutes", "stream_start_rate"},
		ObservationWindowDays: 7,
		PredictionHorizonDays: 90,
		ModelType:             "SURROGATE_MODEL_TYPE_LINEAR",
		CalibrationRSquared:   0.72,
		MlflowModelURI:        "mlflow://models/churn-predictor/1",
	}

	m3 := m5SurrogateToConfig(wire)
	assert.Equal(t, "sm-churn-001", m3.ModelID)
	assert.Equal(t, "churn_7d", m3.TargetMetricID)
	assert.Equal(t, []string{"watch_time_minutes", "stream_start_rate"}, m3.InputMetricIDs)
	assert.Equal(t, 7, m3.ObservationWindowDays)
	assert.Equal(t, 90, m3.PredictionHorizonDays)
	assert.Equal(t, "LINEAR", m3.ModelType,
		"model type must be stripped of SURROGATE_MODEL_TYPE_ prefix")
	assert.InDelta(t, 0.72, m3.CalibrationRSquared, 1e-9)
	assert.Equal(t, "mlflow://models/churn-predictor/1", m3.MLflowModelURI)
}

// ---------------------------------------------------------------------------
// Tests: camelCase ↔ snake_case field naming
// ---------------------------------------------------------------------------

// TestM3M5_VariantFieldNaming verifies that M5's camelCase variant fields
// map to M3's snake_case fields via the wire type definitions.
func TestM3M5_VariantFieldNaming(t *testing.T) {
	// M5 protojson wire format uses camelCase.
	m5Tags := extractJSONTags(reflect.TypeOf(m5WireVariant{}))
	sort.Strings(m5Tags)
	assert.Equal(t, []string{"isControl", "name", "payloadJson", "trafficFraction", "variantId"}, m5Tags)

	// M3 config uses snake_case.
	m3Tags := extractJSONTags(reflect.TypeOf(config.VariantConfig{}))
	sort.Strings(m3Tags)
	assert.Equal(t, []string{"is_control", "name", "traffic_fraction", "variant_id"}, m3Tags)

	// Verify structural compatibility (same field count minus payload_json which M3 doesn't use).
	assert.Equal(t, len(m3Tags)+1, len(m5Tags),
		"M5 has one extra field (payloadJson) not in M3's VariantConfig")
}

// TestM3M5_GuardrailConfigFieldNaming verifies guardrail config field alignment.
func TestM3M5_GuardrailConfigFieldNaming(t *testing.T) {
	m5Tags := extractJSONTags(reflect.TypeOf(m5WireGuardrail{}))
	sort.Strings(m5Tags)
	assert.Equal(t, []string{"consecutiveBreachesRequired", "metricId", "threshold"}, m5Tags)

	m3Tags := extractJSONTags(reflect.TypeOf(config.GuardrailConfig{}))
	sort.Strings(m3Tags)
	assert.Equal(t, []string{"consecutive_breaches_required", "metric_id", "threshold"}, m3Tags)

	// Same field count: perfect structural parity.
	assert.Equal(t, len(m3Tags), len(m5Tags),
		"M3 and M5 GuardrailConfig must have same number of fields")
}

// ---------------------------------------------------------------------------
// Tests: Interleaving experiment wire format
// ---------------------------------------------------------------------------

// TestM3M5_InterleavingExperiment_WireFormat verifies that interleaving-specific
// fields (credit_assignment, engagement_event_type) are correctly nested in
// M5's interleavingConfig vs flat in M3's ExperimentConfig.
func TestM3M5_InterleavingExperiment_WireFormat(t *testing.T) {
	m5 := m5WireExperiment{
		ExperimentID:    "e-interleave",
		Name:            "search_ranking",
		Type:            "EXPERIMENT_TYPE_INTERLEAVING",
		State:           "EXPERIMENT_STATE_RUNNING",
		PrimaryMetricID: "search_success",
		InterleavingConfig: &m5WireInterleave{
			CreditAssignment:    "proportional",
			EngagementEventType: "click",
		},
		Variants: []m5WireVariant{
			{VariantID: "v1", Name: "current", TrafficFraction: 0.5, IsControl: true},
			{VariantID: "v2", Name: "neural", TrafficFraction: 0.5},
		},
	}

	m3 := m5ExperimentToConfig(m5)
	assert.Equal(t, "INTERLEAVING", m3.Type)
	assert.Equal(t, "proportional", m3.CreditAssignment,
		"interleaving credit_assignment must be flattened from interleavingConfig")
	assert.Equal(t, "click", m3.EngagementEventType,
		"interleaving engagement_event_type must be flattened from interleavingConfig")
}

// ---------------------------------------------------------------------------
// Tests: Lifecycle + session config
// ---------------------------------------------------------------------------

// TestM3M5_LifecycleSessionConfig_WireFormat verifies that lifecycle and session
// configs are correctly mapped from M5's nested structures to M3's flat fields.
func TestM3M5_LifecycleSessionConfig_WireFormat(t *testing.T) {
	m5 := m5WireExperiment{
		ExperimentID:    "e-lifecycle",
		Name:            "qoe_test",
		Type:            "EXPERIMENT_TYPE_AB",
		State:           "EXPERIMENT_STATE_RUNNING",
		PrimaryMetricID: "ttff_mean",
		SessionConfig:   &m5WireSession{SessionLevel: true},
		LifecycleConfig: &m5WireLifecycle{
			Enabled:  true,
			Segments: []string{"TRIAL", "NEW", "ESTABLISHED"},
		},
		Variants: []m5WireVariant{
			{VariantID: "v1", Name: "control", TrafficFraction: 0.5, IsControl: true},
			{VariantID: "v2", Name: "treatment", TrafficFraction: 0.5},
		},
	}

	m3 := m5ExperimentToConfig(m5)
	assert.Equal(t, true, m3.SessionLevel,
		"session_level must be flattened from sessionConfig")
	assert.Equal(t, true, m3.LifecycleStratificationEnabled,
		"lifecycle_stratification_enabled must be flattened from lifecycleConfig")
	assert.Equal(t, []string{"TRIAL", "NEW", "ESTABLISHED"}, m3.LifecycleSegments,
		"lifecycle_segments must be flattened from lifecycleConfig")
}

// TestM3M5_NilSubConfigs_WireFormat verifies M3 handles nil sub-configs gracefully.
func TestM3M5_NilSubConfigs_WireFormat(t *testing.T) {
	m5 := m5WireExperiment{
		ExperimentID:    "e-simple",
		Name:            "simple_ab",
		Type:            "EXPERIMENT_TYPE_AB",
		State:           "EXPERIMENT_STATE_RUNNING",
		PrimaryMetricID: "ctr",
		Variants: []m5WireVariant{
			{VariantID: "v1", Name: "control", TrafficFraction: 0.5, IsControl: true},
			{VariantID: "v2", Name: "treatment", TrafficFraction: 0.5},
		},
		// No interleavingConfig, sessionConfig, or lifecycleConfig
	}

	m3 := m5ExperimentToConfig(m5)
	assert.Equal(t, false, m3.SessionLevel, "nil sessionConfig → session_level=false")
	assert.Equal(t, false, m3.LifecycleStratificationEnabled, "nil lifecycleConfig → disabled")
	assert.Empty(t, m3.LifecycleSegments, "nil lifecycleConfig → empty segments")
	assert.Empty(t, m3.CreditAssignment, "nil interleavingConfig → empty credit_assignment")
	assert.Empty(t, m3.EngagementEventType, "nil interleavingConfig → empty engagement_event_type")
}

// ---------------------------------------------------------------------------
// Tests: Timestamp format
// ---------------------------------------------------------------------------

// TestM3M5_TimestampFormat_RFC3339 verifies that M5's RFC3339Nano timestamps
// are passed through correctly (M3 stores them as strings).
func TestM3M5_TimestampFormat_RFC3339(t *testing.T) {
	m5 := m5WireExperiment{
		ExperimentID:    "e-ts",
		Name:            "timestamp_test",
		Type:            "EXPERIMENT_TYPE_AB",
		State:           "EXPERIMENT_STATE_RUNNING",
		PrimaryMetricID: "ctr",
		StartedAt:       "2026-03-01T08:00:00.123456789Z",
		Variants: []m5WireVariant{
			{VariantID: "v1", Name: "control", TrafficFraction: 1.0, IsControl: true},
		},
	}

	m3 := m5ExperimentToConfig(m5)
	assert.Equal(t, "2026-03-01T08:00:00.123456789Z", m3.StartedAt,
		"RFC3339Nano timestamp must be preserved as-is in M3 config")
}

// ---------------------------------------------------------------------------
// Tests: Seed config alignment
// ---------------------------------------------------------------------------

// TestM3M5_SeedConfig_EnumsAreShort verifies that M3's current seed config
// uses short enum strings for experiment-side fields (Experiment.Type, State,
// GuardrailAction) and the proto-direct METRIC_TYPE_* enum for MetricConfig
// (since #506 — MetricConfig embeds *commonv1.MetricDefinition, so seed JSON
// carries the prefixed protojson form).
func TestM3M5_SeedConfig_EnumsAreShort(t *testing.T) {
	cs, err := config.LoadFromFile("config/testdata/seed_config.json")
	require.NoError(t, err)

	exp, err := cs.GetExperiment("e0000000-0000-0000-0000-000000000001")
	require.NoError(t, err)

	assert.Equal(t, "AB", exp.Type,
		"M3 config uses short 'AB', not M5's 'EXPERIMENT_TYPE_AB'")
	assert.Equal(t, "RUNNING", exp.State,
		"M3 config uses short 'RUNNING', not M5's 'EXPERIMENT_STATE_RUNNING'")
	assert.Equal(t, "AUTO_PAUSE", exp.GuardrailAction,
		"M3 config uses short 'AUTO_PAUSE', not M5's 'GUARDRAIL_ACTION_AUTO_PAUSE'")

	metric, err := cs.GetMetric("rebuffer_rate")
	require.NoError(t, err)
	assert.Equal(t, commonv1.MetricType_METRIC_TYPE_RATIO, metric.Type,
		"M3 MetricConfig.Type is the proto-direct typed enum")
}

// TestM3M5_SeedConfig_RunningFilter verifies M3's RunningExperimentIDs filter
// matches the short "RUNNING" string, not M5's prefixed "EXPERIMENT_STATE_RUNNING".
func TestM3M5_SeedConfig_RunningFilter(t *testing.T) {
	cs, err := config.LoadFromFile("config/testdata/seed_config.json")
	require.NoError(t, err)

	running := cs.RunningExperimentIDs()
	assert.NotEmpty(t, running, "seed config must have at least one RUNNING experiment")

	// Verify all returned experiments are actually RUNNING.
	for _, id := range running {
		exp, err := cs.GetExperiment(id)
		require.NoError(t, err)
		assert.Equal(t, "RUNNING", exp.State,
			"RunningExperimentIDs must return experiments with state='RUNNING'")
	}
}

// ---------------------------------------------------------------------------
// Tests: M5 → M3 JSON deserialization
// ---------------------------------------------------------------------------

// TestM3M5_WireJSON_DeserializesToM5Types verifies that a realistic M5 wire-format
// JSON response deserializes correctly into the M5 snapshot types defined above.
func TestM3M5_WireJSON_DeserializesToM5Types(t *testing.T) {
	wireJSON := `{
		"experimentId": "e0000001",
		"name": "homepage_recs_v2",
		"type": "EXPERIMENT_TYPE_AB",
		"state": "EXPERIMENT_STATE_RUNNING",
		"primaryMetricId": "ctr_recommendation",
		"secondaryMetricIds": ["watch_time_minutes"],
		"startedAt": "2026-03-01T08:00:00Z",
		"surrogateModelId": "sm-001",
		"guardrailAction": "GUARDRAIL_ACTION_AUTO_PAUSE",
		"variants": [
			{"variantId": "v1", "name": "control", "trafficFraction": 0.5, "isControl": true},
			{"variantId": "v2", "name": "treatment", "trafficFraction": 0.5}
		],
		"guardrailConfigs": [
			{"metricId": "rebuffer_rate", "threshold": 0.05, "consecutiveBreachesRequired": 3}
		]
	}`

	var exp m5WireExperiment
	err := json.Unmarshal([]byte(wireJSON), &exp)
	require.NoError(t, err)

	assert.Equal(t, "e0000001", exp.ExperimentID)
	assert.Equal(t, "EXPERIMENT_TYPE_AB", exp.Type)
	assert.Equal(t, "EXPERIMENT_STATE_RUNNING", exp.State)
	assert.Equal(t, "ctr_recommendation", exp.PrimaryMetricID)
	assert.Equal(t, []string{"watch_time_minutes"}, exp.SecondaryMetricIDs)
	assert.Equal(t, "GUARDRAIL_ACTION_AUTO_PAUSE", exp.GuardrailAction)
	require.Len(t, exp.Variants, 2)
	assert.Equal(t, true, exp.Variants[0].IsControl)
	assert.Equal(t, false, exp.Variants[1].IsControl)
	require.Len(t, exp.GuardrailConfigs, 1)
	assert.Equal(t, "rebuffer_rate", exp.GuardrailConfigs[0].MetricID)
}

// TestM3M5_MetricWireJSON_DeserializesToM5Type verifies metric definition wire JSON.
func TestM3M5_MetricWireJSON_DeserializesToM5Type(t *testing.T) {
	wireJSON := `{
		"metricId": "latency_p50_ms",
		"name": "Playback Start Latency p50",
		"type": "METRIC_TYPE_PERCENTILE",
		"sourceEventType": "playback_start",
		"percentile": 0.5,
		"lowerIsBetter": true
	}`

	var md m5WireMetricDefinition
	err := json.Unmarshal([]byte(wireJSON), &md)
	require.NoError(t, err)

	assert.Equal(t, "latency_p50_ms", md.MetricID)
	assert.Equal(t, "METRIC_TYPE_PERCENTILE", md.Type)
	assert.InDelta(t, 0.5, md.Percentile, 1e-9)
	assert.Equal(t, true, md.LowerIsBetter)
}

// ---------------------------------------------------------------------------
// Conversion helpers (simulating the M5 → M3 adapter)
// ---------------------------------------------------------------------------

func m5ExperimentToConfig(m5 m5WireExperiment) config.ExperimentConfig {
	cfg := config.ExperimentConfig{
		ExperimentID:       m5.ExperimentID,
		Name:               m5.Name,
		Type:               stripEnumPrefix(m5.Type, m5ExperimentTypePrefix),
		State:              stripEnumPrefix(m5.State, m5ExperimentStatePrefix),
		StartedAt:          m5.StartedAt,
		PrimaryMetricID:    m5.PrimaryMetricID,
		SecondaryMetricIDs: m5.SecondaryMetricIDs,
		GuardrailAction:    stripEnumPrefix(m5.GuardrailAction, m5GuardrailActionPrefix),
		SurrogateModelID:   m5.SurrogateModelID,
	}

	for _, v := range m5.Variants {
		cfg.Variants = append(cfg.Variants, config.VariantConfig{
			VariantID:       v.VariantID,
			Name:            v.Name,
			TrafficFraction: v.TrafficFraction,
			IsControl:       v.IsControl,
		})
	}

	for _, g := range m5.GuardrailConfigs {
		cfg.GuardrailConfigs = append(cfg.GuardrailConfigs, config.GuardrailConfig{
			MetricID:                    g.MetricID,
			Threshold:                   g.Threshold,
			ConsecutiveBreachesRequired: int(g.ConsecutiveBreachesRequired),
		})
	}

	if m5.InterleavingConfig != nil {
		cfg.CreditAssignment = m5.InterleavingConfig.CreditAssignment
		cfg.EngagementEventType = m5.InterleavingConfig.EngagementEventType
	}
	if m5.SessionConfig != nil {
		cfg.SessionLevel = m5.SessionConfig.SessionLevel
	}
	if m5.LifecycleConfig != nil {
		cfg.LifecycleStratificationEnabled = m5.LifecycleConfig.Enabled
		cfg.LifecycleSegments = m5.LifecycleConfig.Segments
	}

	return cfg
}

func m5MetricToConfig(m5 m5WireMetricDefinition) config.MetricConfig {
	// Since #506 the proto fields live on the embedded MetricDefinition.
	// Map enum strings back to typed enums; sibling fields keep their slots.
	md := &commonv1.MetricDefinition{
		MetricId:               m5.MetricID,
		Name:                   m5.Name,
		Type:                   commonv1.MetricType(commonv1.MetricType_value[m5.Type]),
		SourceEventType:        m5.SourceEventType,
		NumeratorEventType:     m5.NumeratorEventType,
		DenominatorEventType:   m5.DenominatorEventType,
		CupedCovariateMetricId: m5.CupedCovariateMetricID,
		Percentile:             m5.Percentile,
		LowerIsBetter:          m5.LowerIsBetter,
		IsQoeMetric:            m5.IsQoEMetric,
		CustomSql:              m5.CustomSQL,
	}

	// ADR-026 Phase 1 — preserve oneof type_config arms directly on the proto.
	if m5.FilteredMean != nil {
		md.TypeConfig = &commonv1.MetricDefinition_FilteredMean{
			FilteredMean: &commonv1.FilteredMeanConfig{
				FilterSql:   m5.FilteredMean.FilterSQL,
				ValueColumn: m5.FilteredMean.ValueColumn,
			},
		}
	}
	if m5.Composite != nil {
		operands := make([]*commonv1.CompositeOperand, len(m5.Composite.Operands))
		for i, op := range m5.Composite.Operands {
			operands[i] = &commonv1.CompositeOperand{MetricId: op.MetricID, Weight: op.Weight}
		}
		md.TypeConfig = &commonv1.MetricDefinition_Composite{
			Composite: &commonv1.CompositeConfig{
				Operands: operands,
				Operator: commonv1.CompositeOperator(commonv1.CompositeOperator_value[m5.Composite.Operator]),
			},
		}
	}
	if m5.WindowedCount != nil {
		md.TypeConfig = &commonv1.MetricDefinition_WindowedCount{
			WindowedCount: &commonv1.WindowedCountConfig{
				EventType:   m5.WindowedCount.EventType,
				FilterSql:   m5.WindowedCount.FilterSQL,
				WindowHours: m5.WindowedCount.WindowHours,
			},
		}
	}
	return config.MetricConfig{MetricDefinition: md}
}

func m5SurrogateToConfig(m5 m5WireSurrogateModel) config.SurrogateModelConfig {
	return config.SurrogateModelConfig{
		ModelID:               m5.ModelID,
		TargetMetricID:        m5.TargetMetricID,
		InputMetricIDs:        m5.InputMetricIDs,
		ObservationWindowDays: int(m5.ObservationWindowDays),
		PredictionHorizonDays: int(m5.PredictionHorizonDays),
		ModelType:             stripEnumPrefix(m5.ModelType, m5SurrogateModelTypePrefix),
		CalibrationRSquared:   m5.CalibrationRSquared,
		MLflowModelURI:        m5.MlflowModelURI,
	}
}

// extractJSONTags returns all JSON field names (without options like omitempty)
// from a struct type. Walks anonymous-embedded structs (and pointers to them)
// so that MetricConfig — which now embeds *commonv1.MetricDefinition — exposes
// the proto-generated JSON tags alongside its own M3-only tags (issue #506).
func extractJSONTags(t reflect.Type) []string {
	var tags []string
	for i := 0; i < t.NumField(); i++ {
		f := t.Field(i)
		if f.Anonymous {
			ft := f.Type
			if ft.Kind() == reflect.Ptr {
				ft = ft.Elem()
			}
			if ft.Kind() == reflect.Struct {
				tags = append(tags, extractJSONTags(ft)...)
				continue
			}
		}
		tag := f.Tag.Get("json")
		if tag == "" || tag == "-" {
			continue
		}
		// Strip ",omitempty" etc.
		if idx := strings.Index(tag, ","); idx != -1 {
			tag = tag[:idx]
		}
		tags = append(tags, tag)
	}
	return tags
}
