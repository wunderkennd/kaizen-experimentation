package alerts

import (
	"sync"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func TestBreachTracker_GetCount_NonExistentKey(t *testing.T) {
	tracker := NewBreachTracker()
	assert.Equal(t, 0, tracker.GetCount("nonexistent", "metric", "variant"))
}

func TestBreachTracker_GetCount_AfterReset(t *testing.T) {
	tracker := NewBreachTracker()
	tracker.RecordCheck("e1", "m1", "v1", true)
	tracker.RecordCheck("e1", "m1", "v1", true)
	assert.Equal(t, 2, tracker.GetCount("e1", "m1", "v1"))

	// Non-breach resets count.
	tracker.RecordCheck("e1", "m1", "v1", false)
	assert.Equal(t, 0, tracker.GetCount("e1", "m1", "v1"))
}

func TestBreachTracker_ManyKeys(t *testing.T) {
	tracker := NewBreachTracker()
	const n = 100

	for i := 0; i < n; i++ {
		expID := "exp-" + string(rune('A'+i%26))
		metricID := "metric-" + string(rune('0'+i%10))
		variantID := "variant-" + string(rune('a'+i%5))
		tracker.RecordCheck(expID, metricID, variantID, true)
	}

	// Spot check a few.
	assert.Greater(t, tracker.GetCount("exp-A", "metric-0", "variant-a"), 0)
}

func TestBreachTracker_ConcurrentAccess(t *testing.T) {
	tracker := NewBreachTracker()
	var wg sync.WaitGroup
	const goroutines = 100

	wg.Add(goroutines)
	for i := 0; i < goroutines; i++ {
		go func(i int) {
			defer wg.Done()
			breached := i%2 == 0
			tracker.RecordCheck("e1", "m1", "v1", breached)
			tracker.GetCount("e1", "m1", "v1")
		}(i)
	}
	wg.Wait()
	// Just verify no panic/race; exact count depends on goroutine ordering.
}

func TestBreachTracker_RecordCheck_ReturnValues(t *testing.T) {
	tracker := NewBreachTracker()

	// Breach returns incrementing count.
	assert.Equal(t, 1, tracker.RecordCheck("e1", "m1", "v1", true))
	assert.Equal(t, 2, tracker.RecordCheck("e1", "m1", "v1", true))
	assert.Equal(t, 3, tracker.RecordCheck("e1", "m1", "v1", true))

	// Non-breach returns 0.
	assert.Equal(t, 0, tracker.RecordCheck("e1", "m1", "v1", false))

	// Next breach starts at 1 again.
	assert.Equal(t, 1, tracker.RecordCheck("e1", "m1", "v1", true))
}

func TestBreachTracker_DifferentVariants(t *testing.T) {
	tracker := NewBreachTracker()

	tracker.RecordCheck("e1", "m1", "control", true)
	tracker.RecordCheck("e1", "m1", "control", true)
	tracker.RecordCheck("e1", "m1", "treatment", true)

	assert.Equal(t, 2, tracker.GetCount("e1", "m1", "control"))
	assert.Equal(t, 1, tracker.GetCount("e1", "m1", "treatment"))

	// Resetting one variant doesn't affect the other.
	tracker.RecordCheck("e1", "m1", "control", false)
	assert.Equal(t, 0, tracker.GetCount("e1", "m1", "control"))
	assert.Equal(t, 1, tracker.GetCount("e1", "m1", "treatment"))
}

func TestBreachTracker_DifferentExperiments(t *testing.T) {
	tracker := NewBreachTracker()

	tracker.RecordCheck("exp-A", "m1", "v1", true)
	tracker.RecordCheck("exp-A", "m1", "v1", true)
	tracker.RecordCheck("exp-B", "m1", "v1", true)

	assert.Equal(t, 2, tracker.GetCount("exp-A", "m1", "v1"))
	assert.Equal(t, 1, tracker.GetCount("exp-B", "m1", "v1"))
}

func TestMemPublisher_ConcurrentPublish(t *testing.T) {
	pub := NewMemPublisher()
	var wg sync.WaitGroup
	const goroutines = 100

	wg.Add(goroutines)
	for i := 0; i < goroutines; i++ {
		go func() {
			defer wg.Done()
			_ = pub.PublishAlert(nil, GuardrailAlert{ExperimentID: "e1"})
		}()
	}
	wg.Wait()

	assert.Len(t, pub.Alerts(), goroutines)
}

func TestMemPublisher_Alerts_ReturnsCopy(t *testing.T) {
	pub := NewMemPublisher()
	_ = pub.PublishAlert(nil, GuardrailAlert{ExperimentID: "e1"})

	alerts1 := pub.Alerts()
	alerts2 := pub.Alerts()
	require.Len(t, alerts1, 1)

	// Mutating returned slice should not affect publisher.
	alerts1[0].ExperimentID = "MODIFIED"
	assert.Equal(t, "e1", alerts2[0].ExperimentID)
	assert.Equal(t, "e1", pub.Alerts()[0].ExperimentID)
}
