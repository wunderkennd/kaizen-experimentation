package state

import "testing"

func TestValidTransitions(t *testing.T) {
	tests := []struct {
		from  State
		to    State
		valid bool
	}{
		{Draft, Starting, true},
		{Starting, Running, true},
		{Starting, Draft, true},
		{Running, Concluding, true},
		{Concluding, Concluded, true},
		{Concluded, Archived, true},
		// Invalid
		{Draft, Running, false},
		{Running, Draft, false},
		{Concluded, Running, false},
		{Archived, Draft, false},
	}

	for _, tt := range tests {
		t.Run(string(tt.from)+"->"+string(tt.to), func(t *testing.T) {
			got := CanTransition(tt.from, tt.to)
			if got != tt.valid {
				t.Errorf("CanTransition(%s, %s) = %v, want %v", tt.from, tt.to, got, tt.valid)
			}
		})
	}
}
