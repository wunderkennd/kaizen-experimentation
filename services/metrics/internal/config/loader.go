package config

import (
	"encoding/json"
	"fmt"
	"os"
	"sync"
)

type VariantConfig struct {
	VariantID      string  `json:"variant_id"`
	Name           string  `json:"name"`
	TrafficFraction float64 `json:"traffic_fraction"`
	IsControl      bool    `json:"is_control"`
}

type GuardrailConfig struct {
	MetricID                   string  `json:"metric_id"`
	Threshold                  float64 `json:"threshold"`
	ConsecutiveBreachesRequired int     `json:"consecutive_breaches_required"`
}

type ExperimentConfig struct {
	ExperimentID                 string            `json:"experiment_id"`
	Name                         string            `json:"name"`
	Type                         string            `json:"type"`
	State                        string            `json:"state"`
	StartedAt                    string            `json:"started_at,omitempty"`
	PrimaryMetricID              string            `json:"primary_metric_id"`
	SecondaryMetricIDs           []string          `json:"secondary_metric_ids"`
	Variants                     []VariantConfig   `json:"variants"`
	GuardrailConfigs             []GuardrailConfig `json:"guardrail_configs,omitempty"`
	GuardrailAction              string            `json:"guardrail_action,omitempty"`
	LifecycleStratificationEnabled bool            `json:"lifecycle_stratification_enabled,omitempty"`
	LifecycleSegments            []string          `json:"lifecycle_segments,omitempty"`
	SurrogateModelID             string            `json:"surrogate_model_id,omitempty"`
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
	Coefficients          map[string]float64 `json:"coefficients,omitempty"`
	Intercept             float64            `json:"intercept,omitempty"`
}

type MetricConfig struct {
	MetricID             string `json:"metric_id"`
	Name                 string `json:"name"`
	Type                 string `json:"type"`
	SourceEventType      string `json:"source_event_type"`
	NumeratorEventType   string `json:"numerator_event_type,omitempty"`
	DenominatorEventType string `json:"denominator_event_type,omitempty"`
	CupedCovariateMetricID string `json:"cuped_covariate_metric_id,omitempty"`
	LowerIsBetter        bool   `json:"lower_is_better,omitempty"`
	IsQoEMetric          bool   `json:"is_qoe_metric,omitempty"`
	QoEField             string `json:"qoe_field,omitempty"`
}

type seedFile struct {
	Experiments     []ExperimentConfig     `json:"experiments"`
	Metrics         []MetricConfig         `json:"metrics"`
	SurrogateModels []SurrogateModelConfig `json:"surrogate_models,omitempty"`
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
	var sf seedFile
	if err := json.Unmarshal(data, &sf); err != nil {
		return nil, fmt.Errorf("config: parse JSON: %w", err)
	}
	cs := &ConfigStore{
		experiments:     make(map[string]*ExperimentConfig, len(sf.Experiments)),
		metrics:         make(map[string]*MetricConfig, len(sf.Metrics)),
		expMetrics:      make(map[string][]string, len(sf.Experiments)),
		surrogateModels: make(map[string]*SurrogateModelConfig, len(sf.SurrogateModels)),
	}
	for i := range sf.Metrics {
		m := sf.Metrics[i]
		cs.metrics[m.MetricID] = &m
	}
	for i := range sf.SurrogateModels {
		sm := sf.SurrogateModels[i]
		cs.surrogateModels[sm.ModelID] = &sm
	}
	for i := range sf.Experiments {
		e := sf.Experiments[i]
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
