// Package synthetic generates test data for acceptance testing of metric computation.
package synthetic

import (
	"fmt"
	"math"
	"math/rand"
)

// Exposure represents a user's assignment to an experiment variant.
type Exposure struct {
	UserID       string
	ExperimentID string
	VariantID    string
}

// MetricEvent represents a single user action.
type MetricEvent struct {
	UserID    string
	EventType string
	Value     float64
}

// GenerateSyntheticData creates deterministic test data with known distributions.
// - numUsers users split 50/50 between control/treatment
// - MEAN metrics: treatment N(10,2), control N(8,2)
// - PROPORTION metrics: treatment 60% conversion, control 50%
// - COUNT metrics: treatment Poisson(5), control Poisson(3)
func GenerateSyntheticData(
	experimentID string,
	controlVariantID string,
	treatmentVariantID string,
	numUsers int,
	eventsPerUser int,
	eventType string,
	metricType string,
) ([]Exposure, []MetricEvent) {
	rng := rand.New(rand.NewSource(42))

	exposures := make([]Exposure, 0, numUsers)
	events := make([]MetricEvent, 0, numUsers*eventsPerUser)

	for i := 0; i < numUsers; i++ {
		userID := fmt.Sprintf("user-%06d", i)
		variantID := controlVariantID
		isControl := true
		if i >= numUsers/2 {
			variantID = treatmentVariantID
			isControl = false
		}

		exposures = append(exposures, Exposure{
			UserID:       userID,
			ExperimentID: experimentID,
			VariantID:    variantID,
		})

		numEvents := eventsPerUser
		switch metricType {
		case "MEAN":
			for j := 0; j < numEvents; j++ {
				var value float64
				if isControl {
					value = rng.NormFloat64()*2 + 8
				} else {
					value = rng.NormFloat64()*2 + 10
				}
				events = append(events, MetricEvent{
					UserID:    userID,
					EventType: eventType,
					Value:     value,
				})
			}
		case "PROPORTION":
			threshold := 0.5
			if !isControl {
				threshold = 0.6
			}
			if rng.Float64() < threshold {
				events = append(events, MetricEvent{
					UserID:    userID,
					EventType: eventType,
					Value:     1.0,
				})
			}
		case "COUNT":
			lambda := 3.0
			if !isControl {
				lambda = 5.0
			}
			n := poissonSample(rng, lambda)
			for j := 0; j < n; j++ {
				events = append(events, MetricEvent{
					UserID:    userID,
					EventType: eventType,
					Value:     1.0,
				})
			}
		}
	}

	return exposures, events
}

// poissonSample generates a Poisson-distributed random number using Knuth's algorithm.
func poissonSample(rng *rand.Rand, lambda float64) int {
	l := math.Exp(-lambda)
	k := 0
	p := 1.0
	for {
		k++
		p *= rng.Float64()
		if p < l {
			break
		}
	}
	return k - 1
}
