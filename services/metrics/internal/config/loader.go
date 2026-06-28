package config

import (
	"encoding/json"
	"fmt"
	"os"
	"strings"
	"sync"

	commonv1 "github.com/org/experimentation/gen/go/experimentation/common/v1"
	"google.golang.org/protobuf/encoding/protojson"
)

type VariantConfig struct {
	VariantID       string  `json:"variant_id"`
	Name            string  `json:"name"`
	TrafficFraction float64 `json:"traffic_fraction"`
	IsControl       bool    `json:"is_control"`
}

type GuardrailConfig struct {
	MetricID                    string  `json:"metric_id"`
	Threshold                   float64 `json:"threshold"`
	ConsecutiveBreachesRequired int     `json:"consecutive_breaches_required"`
}

type ExperimentConfig struct {
	ExperimentID                   string            `json:"experiment_id"`
	Name                           string            `json:"name"`
	Type                           string            `json:"type"`
	State                          string            `json:"state"`
	StartedAt                      string            `json:"started_at,omitempty"`
	PrimaryMetricID                string            `json:"primary_metric_id"`
	SecondaryMetricIDs             []string          `json:"secondary_metric_ids"`
	Variants                       []VariantConfig   `json:"variants"`
	GuardrailConfigs               []GuardrailConfig `json:"guardrail_configs,omitempty"`
	GuardrailAction                string            `json:"guardrail_action,omitempty"`
	LifecycleStratificationEnabled bool              `json:"lifecycle_stratification_enabled,omitempty"`
	LifecycleSegments              []string          `json:"lifecycle_segments,omitempty"`
	SurrogateModelID               string            `json:"surrogate_model_id,omitempty"`
	// Interleaving-specific fields
	CreditAssignment    string `json:"credit_assignment,omitempty"`     // "binary_win", "proportional", or "weighted"
	EngagementEventType string `json:"engagement_event_type,omitempty"` // event_type to join for engagement
	SessionLevel        bool   `json:"session_level,omitempty"`         // whether metrics are session-level
	// MLRATE cross-fitting fields (ADR-015 Phase 2)
	MLRATEEnabled bool `json:"mlrate_enabled,omitempty"` // enable MLRATE cross-fitting for AVLM covariates
	MLRATEFolds   int  `json:"mlrate_folds,omitempty"`   // K-fold count (default 5)
}

type SurrogateModelConfig struct {
	ModelID               string   `json:"model_id"`
	TargetMetricID        string   `json:"target_metric_id"`
	InputMetricIDs        []string `json:"input_metric_ids"`
	ObservationWindowDays int      `json:"observation_window_days"`
	PredictionHorizonDays int      `json:"prediction_horizon_days"`
	ModelType             string   `json:"model_type"` // LINEAR, GRADIENT_BOOSTED, NEURAL
	CalibrationRSquared   float64  `json:"calibration_r_squared"`
	MLflowModelURI        string   `json:"mlflow_model_uri,omitempty"`
	// For mock linear models: coefficients per input metric
	Coefficients map[string]float64 `json:"coefficients,omitempty"`
	Intercept    float64            `json:"intercept,omitempty"`
}

// MetricConfig embeds the proto MetricDefinition so any field added to the proto
// is automatically readable by M3 without code changes (issue #506). The four
// trailing fields are M3-only and have no proto counterpart — they are loaded
// from seed JSON in parallel with the protojson-parsed proto half.
type MetricConfig struct {
	*commonv1.MetricDefinition

	// M3-only fields (not in proto MetricDefinition):
	QoEField                string   `json:"qoe_field,omitempty"`
	MLRATEFeatureEventTypes []string `json:"mlrate_feature_event_types,omitempty"`
	MLRATEModelURI          string   `json:"mlrate_model_uri,omitempty"`
	MLRATELookbackDays      int      `json:"mlrate_lookback_days,omitempty"`
}

// TypeShortName returns the short form of a MetricType enum, stripping the
// "METRIC_TYPE_" prefix. Use for string comparison in renderer/job switches
// where the long form is awkward.
func TypeShortName(t commonv1.MetricType) string {
	return strings.TrimPrefix(t.String(), "METRIC_TYPE_")
}

// CompositeOperatorShortName strips the "COMPOSITE_OPERATOR_" prefix. Returns
// "" for the UNSPECIFIED zero value so callers don't leak a stale token into
// Spark params or logs when the operator field is absent.
func CompositeOperatorShortName(op commonv1.CompositeOperator) string {
	if op == commonv1.CompositeOperator_COMPOSITE_OPERATOR_UNSPECIFIED {
		return ""
	}
	return strings.TrimPrefix(op.String(), "COMPOSITE_OPERATOR_")
}

type ConfigStore struct {
	mu              sync.RWMutex
	experiments     map[string]*ExperimentConfig
	metrics         map[string]*MetricConfig
	expMetrics      map[string][]string
	surrogateModels map[string]*SurrogateModelConfig
}

func LoadFromFile(path string) (*ConfigStore, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, fmt.Errorf("config: read file %s: %w", path, err)
	}

	// Top-level seed file shape (experiments + surrogate_models still use
	// encoding/json into existing Go structs; only the metrics array gets the
	// proto-direct treatment).
	var top struct {
		Experiments     []ExperimentConfig     `json:"experiments"`
		Metrics         []json.RawMessage      `json:"metrics"`
		SurrogateModels []SurrogateModelConfig `json:"surrogate_models,omitempty"`
	}
	if err := json.Unmarshal(data, &top); err != nil {
		return nil, fmt.Errorf("config: parse JSON: %w", err)
	}

	cs := &ConfigStore{
		experiments:     make(map[string]*ExperimentConfig, len(top.Experiments)),
		metrics:         make(map[string]*MetricConfig, len(top.Metrics)),
		expMetrics:      make(map[string][]string, len(top.Experiments)),
		surrogateModels: make(map[string]*SurrogateModelConfig, len(top.SurrogateModels)),
	}

	// Use DiscardUnknown because seed entries carry the four M3-only fields
	// (qoe_field, mlrate_*) that proto MetricDefinition does not know about.
	protoOpts := protojson.UnmarshalOptions{DiscardUnknown: true}

	for i, raw := range top.Metrics {
		md := &commonv1.MetricDefinition{}
		if err := protoOpts.Unmarshal(raw, md); err != nil {
			return nil, fmt.Errorf("config: parse metric[%d] via protojson: %w", i, err)
		}
		var m3 struct {
			QoEField                string   `json:"qoe_field"`
			MLRATEFeatureEventTypes []string `json:"mlrate_feature_event_types"`
			MLRATEModelURI          string   `json:"mlrate_model_uri"`
			MLRATELookbackDays      int      `json:"mlrate_lookback_days"`
		}
		if err := json.Unmarshal(raw, &m3); err != nil {
			return nil, fmt.Errorf("config: parse metric[%d] m3-only fields: %w", i, err)
		}
		cs.metrics[md.GetMetricId()] = &MetricConfig{
			MetricDefinition:        md,
			QoEField:                m3.QoEField,
			MLRATEFeatureEventTypes: m3.MLRATEFeatureEventTypes,
			MLRATEModelURI:          m3.MLRATEModelURI,
			MLRATELookbackDays:      m3.MLRATELookbackDays,
		}
	}

	for i := range top.SurrogateModels {
		sm := top.SurrogateModels[i]
		cs.surrogateModels[sm.ModelID] = &sm
	}
	for i := range top.Experiments {
		e := top.Experiments[i]
		cs.experiments[e.ExperimentID] = &e
		metricIDs := make([]string, 0, 1+len(e.SecondaryMetricIDs))
		metricIDs = append(metricIDs, e.PrimaryMetricID)
		metricIDs = append(metricIDs, e.SecondaryMetricIDs...)
		cs.expMetrics[e.ExperimentID] = metricIDs
	}
	return cs, nil
}

func (c *ConfigStore) GetExperiment(id string) (*ExperimentConfig, error) {
	c.mu.RLock()
	defer c.mu.RUnlock()
	e, ok := c.experiments[id]
	if !ok {
		return nil, fmt.Errorf("config: experiment %q not found", id)
	}
	return e, nil
}

func (c *ConfigStore) GetMetric(id string) (*MetricConfig, error) {
	c.mu.RLock()
	defer c.mu.RUnlock()
	m, ok := c.metrics[id]
	if !ok {
		return nil, fmt.Errorf("config: metric %q not found", id)
	}
	return m, nil
}

// GetMetricsForExperiment returns the MetricConfigs for an experiment.
//
// IMPORTANT — shallow-copy semantics: `MetricConfig` embeds
// `*commonv1.MetricDefinition` (a pointer), so the value-copy at the
// `append` below shares the underlying proto message with the
// ConfigStore's map. The same applies to `standard.go` callers
// (`m := *mPtr`).
//
// Current callers are read-only — they invoke proto getters, never
// setters or `proto.Reset` — which keeps this safe in practice. A
// future caller that needs to MUTATE a returned MetricConfig must
// `proto.Clone(m.MetricDefinition)` first, otherwise the mutation
// will corrupt the store. (Devin PR #610 📝 shallow-pointer-copy
// caveat.)
func (c *ConfigStore) GetMetricsForExperiment(id string) ([]MetricConfig, error) {
	c.mu.RLock()
	defer c.mu.RUnlock()
	metricIDs, ok := c.expMetrics[id]
	if !ok {
		return nil, fmt.Errorf("config: experiment %q not found", id)
	}
	result := make([]MetricConfig, 0, len(metricIDs))
	for _, mid := range metricIDs {
		m, ok := c.metrics[mid]
		if !ok {
			return nil, fmt.Errorf("config: metric %q referenced by experiment %q not found", mid, id)
		}
		result = append(result, *m)
	}
	return result, nil
}

func (c *ConfigStore) GetGuardrailsForExperiment(id string) ([]GuardrailConfig, error) {
	c.mu.RLock()
	defer c.mu.RUnlock()
	e, ok := c.experiments[id]
	if !ok {
		return nil, fmt.Errorf("config: experiment %q not found", id)
	}
	return e.GuardrailConfigs, nil
}

// ControlVariantID returns the variant_id of the control variant, or empty string if none.
func (e *ExperimentConfig) ControlVariantID() string {
	for _, v := range e.Variants {
		if v.IsControl {
			return v.VariantID
		}
	}
	return ""
}

// MLRATEFoldsOrDefault returns the configured fold count, or 5 if unset.
func (e *ExperimentConfig) MLRATEFoldsOrDefault() int {
	if e.MLRATEFolds > 0 {
		return e.MLRATEFolds
	}
	return 5
}

// MLRATELookbackDaysOrDefault returns the configured lookback, or 14 if unset.
func (m *MetricConfig) MLRATELookbackDaysOrDefault() int {
	if m.MLRATELookbackDays > 0 {
		return m.MLRATELookbackDays
	}
	return 14
}

func (c *ConfigStore) GetSurrogateModel(id string) (*SurrogateModelConfig, error) {
	c.mu.RLock()
	defer c.mu.RUnlock()
	sm, ok := c.surrogateModels[id]
	if !ok {
		return nil, fmt.Errorf("config: surrogate model %q not found", id)
	}
	return sm, nil
}

// GetSurrogateModelForExperiment returns the surrogate model linked to an experiment, or nil if none.
func (c *ConfigStore) GetSurrogateModelForExperiment(experimentID string) *SurrogateModelConfig {
	c.mu.RLock()
	defer c.mu.RUnlock()
	e, ok := c.experiments[experimentID]
	if !ok || e.SurrogateModelID == "" {
		return nil
	}
	sm, ok := c.surrogateModels[e.SurrogateModelID]
	if !ok {
		return nil
	}
	return sm
}

// GetExperimentsByModelID returns experiment IDs that reference the given surrogate model ID.
// Returns an empty slice if no experiments use the model.
func (c *ConfigStore) GetExperimentsByModelID(modelID string) []string {
	c.mu.RLock()
	defer c.mu.RUnlock()
	var ids []string
	for _, e := range c.experiments {
		if e.SurrogateModelID == modelID {
			ids = append(ids, e.ExperimentID)
		}
	}
	return ids
}

func (c *ConfigStore) RunningExperimentIDs() []string {
	c.mu.RLock()
	defer c.mu.RUnlock()
	var ids []string
	for _, e := range c.experiments {
		if e.State == "RUNNING" {
			ids = append(ids, e.ExperimentID)
		}
	}
	return ids
}
