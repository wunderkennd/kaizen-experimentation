// Package config loads experiment and metric definitions from a local JSON file.
// This mocks M5 (Experiment Management) until it delivers a real config API.
package config

import (
	"encoding/json"
	"fmt"
	"os"
	"sync"
)

// VariantConfig describes a single experiment variant.
type VariantConfig struct {
	VariantID      string  `json:"variant_id"`
	Name           string  `json:"name"`
	TrafficFraction float64 `json:"traffic_fraction"`
	IsControl      bool    `json:"is_control"`
}

// ExperimentConfig describes a single experiment.
type ExperimentConfig struct {
	ExperimentID       string          `json:"experiment_id"`
	Name               string          `json:"name"`
	Type               string          `json:"type"`
	State              string          `json:"state"`
	PrimaryMetricID    string          `json:"primary_metric_id"`
	SecondaryMetricIDs []string        `json:"secondary_metric_ids"`
	Variants           []VariantConfig `json:"variants"`
}

// MetricConfig describes a metric definition.
type MetricConfig struct {
	MetricID        string `json:"metric_id"`
	Name            string `json:"name"`
	Type            string `json:"type"` // MEAN, PROPORTION, COUNT, RATIO, PERCENTILE, CUSTOM
	SourceEventType string `json:"source_event_type"`
}

// seedFile is the top-level JSON structure.
type seedFile struct {
	Experiments []ExperimentConfig `json:"experiments"`
	Metrics     []MetricConfig     `json:"metrics"`
}

// ConfigStore holds experiment and metric configs in memory.
type ConfigStore struct {
	mu          sync.RWMutex
	experiments map[string]*ExperimentConfig
	metrics     map[string]*MetricConfig
	// expMetrics maps experiment_id → list of metric IDs (primary + secondary).
	expMetrics map[string][]string
}

// LoadFromFile reads a seed JSON file and returns a ConfigStore.
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
		experiments: make(map[string]*ExperimentConfig, len(sf.Experiments)),
		metrics:     make(map[string]*MetricConfig, len(sf.Metrics)),
		expMetrics:  make(map[string][]string, len(sf.Experiments)),
	}

	for i := range sf.Metrics {
		m := sf.Metrics[i]
		cs.metrics[m.MetricID] = &m
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

// GetExperiment returns the config for a single experiment.
func (c *ConfigStore) GetExperiment(id string) (*ExperimentConfig, error) {
	c.mu.RLock()
	defer c.mu.RUnlock()
	e, ok := c.experiments[id]
	if !ok {
		return nil, fmt.Errorf("config: experiment %q not found", id)
	}
	return e, nil
}

// GetMetric returns a single metric definition.
func (c *ConfigStore) GetMetric(id string) (*MetricConfig, error) {
	c.mu.RLock()
	defer c.mu.RUnlock()
	m, ok := c.metrics[id]
	if !ok {
		return nil, fmt.Errorf("config: metric %q not found", id)
	}
	return m, nil
}

// GetMetricsForExperiment returns all metrics (primary + secondary) for an experiment.
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

// RunningExperimentIDs returns IDs of all experiments in RUNNING state.
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
