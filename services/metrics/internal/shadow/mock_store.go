package shadow

import (
	"context"
	"encoding/json"
	"fmt"
	"sync"
	"time"

	"github.com/google/uuid"
)

// MockStore is an in-memory Store for unit tests.
// Modelled on services/metrics/internal/querylog/writer.go (MemWriter).
type MockStore struct {
	mu      sync.Mutex
	runs    map[uuid.UUID]*Run
	results []ResultRow
	// transitionErr, when non-nil, is returned by Transition instead of
	// performing the CAS.  Use SetTransitionErr to inject a transient error.
	transitionErr error
}

// NewMockStore returns an empty MockStore.
func NewMockStore() *MockStore {
	return &MockStore{
		runs: make(map[uuid.UUID]*Run),
	}
}

// Schedule inserts a new PENDING run and returns its UUID.
func (m *MockStore) Schedule(_ context.Context, originalMetricID string, candidate json.RawMessage) (uuid.UUID, error) {
	m.mu.Lock()
	defer m.mu.Unlock()
	id := uuid.New()
	raw := make(json.RawMessage, len(candidate))
	copy(raw, candidate)
	m.runs[id] = &Run{
		ShadowID:         id,
		OriginalMetricID: originalMetricID,
		CandidateMetric:  raw,
		ScheduledAt:      time.Now(),
		Status:           StatusPending,
	}
	return id, nil
}

// Get returns a copy of the Run for shadowID, or nil if not found.
func (m *MockStore) Get(_ context.Context, shadowID uuid.UUID) (*Run, error) {
	m.mu.Lock()
	defer m.mu.Unlock()
	r, ok := m.runs[shadowID]
	if !ok {
		return nil, nil
	}
	copy := *r
	return &copy, nil
}

// ListPending returns copies of all PENDING runs.
func (m *MockStore) ListPending(_ context.Context) ([]Run, error) {
	m.mu.Lock()
	defer m.mu.Unlock()
	var out []Run
	for _, r := range m.runs {
		if r.Status == StatusPending {
			cp := *r
			out = append(out, cp)
		}
	}
	return out, nil
}

// Transition performs the CAS update.  Returns an error wrapping ErrCASFailure
// if the row is absent or not in the expected `from` state.  If a transient
// error was injected via SetTransitionErr, that error is returned instead.
func (m *MockStore) Transition(_ context.Context, shadowID uuid.UUID, from, to Status, reason string) error {
	m.mu.Lock()
	defer m.mu.Unlock()
	// Injected transient error takes precedence (for testing CodeInternal path).
	if m.transitionErr != nil {
		return m.transitionErr
	}
	r, ok := m.runs[shadowID]
	if !ok {
		return fmt.Errorf("transition shadow %s %s->%s: %w", shadowID, from, to, ErrCASFailure)
	}
	if r.Status != from {
		return fmt.Errorf("transition shadow %s %s->%s: %w", shadowID, from, to, ErrCASFailure)
	}
	r.Status = to
	if to == StatusRejected || to == StatusFailed {
		r.RejectionReason = reason
	} else {
		r.RejectionReason = ""
	}
	return nil
}

// Results returns all ResultRows for shadowID ordered by insertion order.
func (m *MockStore) Results(_ context.Context, shadowID uuid.UUID) ([]ResultRow, error) {
	m.mu.Lock()
	defer m.mu.Unlock()
	var out []ResultRow
	for _, r := range m.results {
		if r.ShadowID == shadowID {
			out = append(out, r)
		}
	}
	return out, nil
}

// InsertResult appends a result row.
func (m *MockStore) InsertResult(_ context.Context, row ResultRow) error {
	m.mu.Lock()
	defer m.mu.Unlock()
	if row.ResultID == uuid.Nil {
		row.ResultID = uuid.New()
	}
	m.results = append(m.results, row)
	return nil
}

// SetStatus directly sets the status of an existing run (test helper).
func (m *MockStore) SetStatus(shadowID uuid.UUID, s Status) {
	m.mu.Lock()
	defer m.mu.Unlock()
	if r, ok := m.runs[shadowID]; ok {
		r.Status = s
	}
}

// AllRuns returns copies of all runs (test helper).
func (m *MockStore) AllRuns() []Run {
	m.mu.Lock()
	defer m.mu.Unlock()
	var out []Run
	for _, r := range m.runs {
		cp := *r
		out = append(out, cp)
	}
	return out
}

// AllResults returns copies of all result rows (test helper).
func (m *MockStore) AllResults() []ResultRow {
	m.mu.Lock()
	defer m.mu.Unlock()
	out := make([]ResultRow, len(m.results))
	copy(out, m.results)
	return out
}

// SetTransitionErr injects a transient error to be returned by Transition
// instead of performing the CAS.  Pass nil to clear the injection.
// Use this to test the CodeInternal path in PromoteShadowResult.
func (m *MockStore) SetTransitionErr(err error) {
	m.mu.Lock()
	defer m.mu.Unlock()
	m.transitionErr = err
}

// SetRejectionReason sets the RejectionReason on a run (test helper).
// Used to pre-seed a REJECTED/FAILED run with a known reason.
func (m *MockStore) SetRejectionReason(shadowID uuid.UUID, reason string) {
	m.mu.Lock()
	defer m.mu.Unlock()
	if r, ok := m.runs[shadowID]; ok {
		r.RejectionReason = reason
	}
}
