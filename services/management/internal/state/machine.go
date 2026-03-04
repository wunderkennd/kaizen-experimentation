// Package state implements the experiment lifecycle state machine.
// See ADR-005 for transitional state rationale.
package state

import "fmt"

// State represents an experiment lifecycle state.
type State string

const (
	Draft      State = "DRAFT"
	Starting   State = "STARTING"
	Running    State = "RUNNING"
	Concluding State = "CONCLUDING"
	Concluded  State = "CONCLUDED"
	Archived   State = "ARCHIVED"
)

// validTransitions defines the allowed state transitions.
// Key: current state, Value: set of valid next states.
var validTransitions = map[State][]State{
	Draft:      {Starting},
	Starting:   {Running, Draft}, // Back to Draft on validation failure
	Running:    {Concluding, Running}, // Running -> Running = pause/resume (traffic change)
	Concluding: {Concluded},
	Concluded:  {Archived},
	Archived:   {}, // Terminal
}

// CanTransition checks if moving from 'from' to 'to' is valid.
func CanTransition(from, to State) bool {
	allowed, ok := validTransitions[from]
	if !ok {
		return false
	}
	for _, s := range allowed {
		if s == to {
			return true
		}
	}
	return false
}

// Transition validates and returns the new state, or an error.
func Transition(from, to State) error {
	if !CanTransition(from, to) {
		return fmt.Errorf("invalid state transition: %s -> %s", from, to)
	}
	return nil
}
