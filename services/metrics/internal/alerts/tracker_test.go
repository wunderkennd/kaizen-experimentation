package alerts

import (
	"testing"
	"github.com/stretchr/testify/assert"
)

func TestBreachTracker_ConsecutiveBreaches(t *testing.T) {
	tracker := NewBreachTracker()
	assert.Equal(t, 1, tracker.RecordCheck("e1", "m1", "v1", true))
	assert.Equal(t, 2, tracker.RecordCheck("e1", "m1", "v1", true))
	assert.Equal(t, 3, tracker.RecordCheck("e1", "m1", "v1", true))
}

func TestBreachTracker_ResetOnNonBreach(t *testing.T) {
	tracker := NewBreachTracker()
	tracker.RecordCheck("e1", "m1", "v1", true)
	tracker.RecordCheck("e1", "m1", "v1", true)
	assert.Equal(t, 0, tracker.RecordCheck("e1", "m1", "v1", false))
	assert.Equal(t, 1, tracker.RecordCheck("e1", "m1", "v1", true))
}

func TestBreachTracker_IndependentKeys(t *testing.T) {
	tracker := NewBreachTracker()
	tracker.RecordCheck("e1", "m1", "v1", true)
	tracker.RecordCheck("e1", "m1", "v1", true)
	tracker.RecordCheck("e1", "m2", "v1", true)
	assert.Equal(t, 2, tracker.GetCount("e1", "m1", "v1"))
	assert.Equal(t, 1, tracker.GetCount("e1", "m2", "v1"))
	assert.Equal(t, 0, tracker.GetCount("e1", "m1", "v2"))
}
