package jobs

import (
	"context"
	"fmt"
	"sync"
)

type MockValueProvider struct {
	mu     sync.Mutex
	values map[string]map[string]float64
}

func NewMockValueProvider() *MockValueProvider {
	return &MockValueProvider{values: make(map[string]map[string]float64)}
}

func (m *MockValueProvider) SetVariantValue(metricID, variantID string, value float64) {
	m.mu.Lock()
	defer m.mu.Unlock()
	if m.values[metricID] == nil {
		m.values[metricID] = make(map[string]float64)
	}
	m.values[metricID][variantID] = value
}

func (m *MockValueProvider) GetVariantValues(_ context.Context, _ string, metricID string) (map[string]float64, error) {
	m.mu.Lock()
	defer m.mu.Unlock()
	vals, ok := m.values[metricID]
	if !ok {
		return nil, fmt.Errorf("mock: no values configured for metric %q", metricID)
	}
	result := make(map[string]float64, len(vals))
	for k, v := range vals {
		result[k] = v
	}
	return result, nil
}
