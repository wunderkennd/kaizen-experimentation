package status

import (
	"context"
	"sync"
)

// MockWriter is an in-memory Writer for unit tests.
type MockWriter struct {
	mu      sync.Mutex
	Entries []Entry
}

// NewMockWriter returns an empty in-memory Writer.
func NewMockWriter() *MockWriter {
	return &MockWriter{Entries: nil}
}

// Write appends the entry to the in-memory log.
func (w *MockWriter) Write(_ context.Context, entry Entry) error {
	w.mu.Lock()
	defer w.mu.Unlock()
	w.Entries = append(w.Entries, entry)
	return nil
}

// Snapshot returns a copy of the recorded entries (safe to inspect from tests
// without racing the writer).
func (w *MockWriter) Snapshot() []Entry {
	w.mu.Lock()
	defer w.mu.Unlock()
	out := make([]Entry, len(w.Entries))
	copy(out, w.Entries)
	return out
}
