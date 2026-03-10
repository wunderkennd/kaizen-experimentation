package store

import (
	"context"
	"sort"
	"sync"
	"time"

	"github.com/google/uuid"
)

// MockAuditStore is an in-memory AuditStore for testing.
type MockAuditStore struct {
	mu        sync.RWMutex
	entries   []*AuditEntry
	flagStore *MockStore
}

// NewMockAuditStore creates a new in-memory audit store.
func NewMockAuditStore(flagStore *MockStore) *MockAuditStore {
	return &MockAuditStore{
		flagStore: flagStore,
	}
}

func (m *MockAuditStore) RecordAudit(ctx context.Context, entry *AuditEntry) error {
	m.mu.Lock()
	defer m.mu.Unlock()

	recorded := *entry
	recorded.AuditID = uuid.New().String()
	recorded.CreatedAt = time.Now()
	m.entries = append(m.entries, &recorded)
	return nil
}

func (m *MockAuditStore) GetFlagAuditLog(ctx context.Context, flagID string, limit int) ([]*AuditEntry, error) {
	m.mu.RLock()
	defer m.mu.RUnlock()

	if limit <= 0 || limit > 1000 {
		limit = 100
	}

	var result []*AuditEntry
	for _, e := range m.entries {
		if e.FlagID == flagID {
			entry := *e
			result = append(result, &entry)
		}
	}

	sort.Slice(result, func(i, j int) bool {
		return result[i].CreatedAt.After(result[j].CreatedAt)
	})

	if len(result) > limit {
		result = result[:limit]
	}
	return result, nil
}

func (m *MockAuditStore) GetStaleFlags(ctx context.Context, staleThreshold time.Duration) ([]*StaleFlagEntry, error) {
	m.flagStore.mu.RLock()
	defer m.flagStore.mu.RUnlock()

	cutoff := time.Now().Add(-staleThreshold)
	var result []*StaleFlagEntry
	for _, f := range m.flagStore.flags {
		if f.Enabled && f.RolloutPercentage >= 1.0 && f.PromotedExperimentID == "" && f.UpdatedAt.Before(cutoff) {
			result = append(result, &StaleFlagEntry{
				FlagID:            f.FlagID,
				Name:              f.Name,
				Description:       f.Description,
				Type:              f.Type,
				Enabled:           f.Enabled,
				RolloutPercentage: f.RolloutPercentage,
				UpdatedAt:         f.UpdatedAt,
				StaleDuration:     time.Since(f.UpdatedAt),
			})
		}
	}

	sort.Slice(result, func(i, j int) bool {
		return result[i].UpdatedAt.Before(result[j].UpdatedAt)
	})
	return result, nil
}

// Entries returns all recorded audit entries (for test assertions).
func (m *MockAuditStore) Entries() []*AuditEntry {
	m.mu.RLock()
	defer m.mu.RUnlock()
	result := make([]*AuditEntry, len(m.entries))
	copy(result, m.entries)
	return result
}
