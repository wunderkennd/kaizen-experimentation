package spark

import (
	"context"
	"sync"
	"time"
)

// MockCall records a single SQL execution call.
type MockCall struct {
	SQL         string
	TargetTable string
	Result      *SQLResult
}

// MockExecutor is a test double for SQLExecutor that records calls and returns
// pre-configured results. It avoids CGo dependencies (DuckDB).
type MockExecutor struct {
	mu    sync.Mutex
	Calls []MockCall

	// DefaultRowCount is returned when no specific result is configured.
	DefaultRowCount int64
}

// NewMockExecutor creates a MockExecutor with the given default row count.
func NewMockExecutor(defaultRowCount int64) *MockExecutor {
	return &MockExecutor{
		DefaultRowCount: defaultRowCount,
	}
}

func (m *MockExecutor) ExecuteSQL(ctx context.Context, sql string) (*SQLResult, error) {
	result := &SQLResult{
		RowCount: m.DefaultRowCount,
		Duration: 50 * time.Millisecond,
	}
	m.mu.Lock()
	m.Calls = append(m.Calls, MockCall{SQL: sql, Result: result})
	m.mu.Unlock()
	return result, nil
}

func (m *MockExecutor) ExecuteAndWrite(ctx context.Context, sql string, targetTable string) (*SQLResult, error) {
	result := &SQLResult{
		RowCount: m.DefaultRowCount,
		Duration: 100 * time.Millisecond,
	}
	m.mu.Lock()
	m.Calls = append(m.Calls, MockCall{SQL: sql, TargetTable: targetTable, Result: result})
	m.mu.Unlock()
	return result, nil
}

// GetCalls returns a copy of recorded calls (thread-safe).
func (m *MockExecutor) GetCalls() []MockCall {
	m.mu.Lock()
	defer m.mu.Unlock()
	out := make([]MockCall, len(m.Calls))
	copy(out, m.Calls)
	return out
}

// Reset clears all recorded calls.
func (m *MockExecutor) Reset() {
	m.mu.Lock()
	defer m.mu.Unlock()
	m.Calls = nil
}
