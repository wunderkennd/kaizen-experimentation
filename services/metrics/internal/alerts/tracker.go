package alerts

import "sync"

type breachKey struct {
	ExperimentID string
	MetricID     string
	VariantID    string
}

type BreachTracker struct {
	mu     sync.Mutex
	counts map[breachKey]int
}

func NewBreachTracker() *BreachTracker {
	return &BreachTracker{counts: make(map[breachKey]int)}
}

func (t *BreachTracker) RecordCheck(experimentID, metricID, variantID string, breached bool) int {
	t.mu.Lock()
	defer t.mu.Unlock()
	key := breachKey{ExperimentID: experimentID, MetricID: metricID, VariantID: variantID}
	if breached {
		t.counts[key]++
		return t.counts[key]
	}
	delete(t.counts, key)
	return 0
}

func (t *BreachTracker) GetCount(experimentID, metricID, variantID string) int {
	t.mu.Lock()
	defer t.mu.Unlock()
	return t.counts[breachKey{ExperimentID: experimentID, MetricID: metricID, VariantID: variantID}]
}
