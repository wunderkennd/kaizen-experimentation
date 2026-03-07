package store

import (
	"context"
	"encoding/base64"
	"fmt"
	"sort"
	"sync"
	"time"

	"crypto/rand"
	"encoding/hex"

	"github.com/google/uuid"
)

// MockStore is an in-memory Store for testing.
type MockStore struct {
	mu    sync.RWMutex
	flags map[string]*Flag // flagID -> Flag
}

// NewMockStore creates a new in-memory store.
func NewMockStore() *MockStore {
	return &MockStore{
		flags: make(map[string]*Flag),
	}
}

func (m *MockStore) CreateFlag(ctx context.Context, f *Flag) (*Flag, error) {
	m.mu.Lock()
	defer m.mu.Unlock()

	// Check unique name.
	for _, existing := range m.flags {
		if existing.Name == f.Name {
			return nil, fmt.Errorf("flag name already exists: %s", f.Name)
		}
	}

	now := time.Now()
	created := &Flag{
		FlagID:            uuid.New().String(),
		Name:              f.Name,
		Description:       f.Description,
		Type:              f.Type,
		DefaultValue:      f.DefaultValue,
		Enabled:           f.Enabled,
		RolloutPercentage: f.RolloutPercentage,
		Salt:              generateSalt(),
		TargetingRuleID:   f.TargetingRuleID,
		CreatedAt:         now,
		UpdatedAt:         now,
	}

	// Copy variants with generated IDs.
	for i, v := range f.Variants {
		created.Variants = append(created.Variants, FlagVariant{
			VariantID:       uuid.New().String(),
			FlagID:          created.FlagID,
			Value:           v.Value,
			TrafficFraction: v.TrafficFraction,
			Ordinal:         i,
		})
	}

	m.flags[created.FlagID] = created
	return copyFlag(created), nil
}

func (m *MockStore) GetFlag(ctx context.Context, flagID string) (*Flag, error) {
	m.mu.RLock()
	defer m.mu.RUnlock()

	f, ok := m.flags[flagID]
	if !ok {
		return nil, fmt.Errorf("flag not found: %s", flagID)
	}
	return copyFlag(f), nil
}

func (m *MockStore) UpdateFlag(ctx context.Context, f *Flag) (*Flag, error) {
	m.mu.Lock()
	defer m.mu.Unlock()

	existing, ok := m.flags[f.FlagID]
	if !ok {
		return nil, fmt.Errorf("flag not found: %s", f.FlagID)
	}

	// Check name uniqueness (excluding self).
	for _, other := range m.flags {
		if other.FlagID != f.FlagID && other.Name == f.Name {
			return nil, fmt.Errorf("flag name already exists: %s", f.Name)
		}
	}

	existing.Name = f.Name
	existing.Description = f.Description
	existing.Type = f.Type
	existing.DefaultValue = f.DefaultValue
	existing.Enabled = f.Enabled
	existing.RolloutPercentage = f.RolloutPercentage
	existing.TargetingRuleID = f.TargetingRuleID
	existing.UpdatedAt = time.Now()

	// Replace variants.
	existing.Variants = nil
	for i, v := range f.Variants {
		existing.Variants = append(existing.Variants, FlagVariant{
			VariantID:       uuid.New().String(),
			FlagID:          existing.FlagID,
			Value:           v.Value,
			TrafficFraction: v.TrafficFraction,
			Ordinal:         i,
		})
	}

	return copyFlag(existing), nil
}

func (m *MockStore) DeleteFlag(ctx context.Context, flagID string) error {
	m.mu.Lock()
	defer m.mu.Unlock()

	if _, ok := m.flags[flagID]; !ok {
		return fmt.Errorf("flag not found: %s", flagID)
	}
	delete(m.flags, flagID)
	return nil
}

func (m *MockStore) ListFlags(ctx context.Context, pageSize int, pageToken string) ([]*Flag, string, error) {
	m.mu.RLock()
	defer m.mu.RUnlock()

	if pageSize <= 0 || pageSize > 100 {
		pageSize = 50
	}

	// Collect and sort by ID.
	var all []*Flag
	for _, f := range m.flags {
		all = append(all, f)
	}
	sort.Slice(all, func(i, j int) bool { return all[i].FlagID < all[j].FlagID })

	// Apply cursor.
	var cursor string
	if pageToken != "" {
		decoded, err := base64.StdEncoding.DecodeString(pageToken)
		if err != nil {
			return nil, "", fmt.Errorf("invalid page token")
		}
		cursor = string(decoded)
	}

	var result []*Flag
	for _, f := range all {
		if cursor != "" && f.FlagID <= cursor {
			continue
		}
		result = append(result, copyFlag(f))
		if len(result) > pageSize {
			break
		}
	}

	var nextToken string
	if len(result) > pageSize {
		nextToken = base64.StdEncoding.EncodeToString([]byte(result[pageSize-1].FlagID))
		result = result[:pageSize]
	}

	return result, nextToken, nil
}

func (m *MockStore) GetAllEnabledFlags(ctx context.Context) ([]*Flag, error) {
	m.mu.RLock()
	defer m.mu.RUnlock()

	var result []*Flag
	for _, f := range m.flags {
		if f.Enabled {
			result = append(result, copyFlag(f))
		}
	}
	sort.Slice(result, func(i, j int) bool { return result[i].FlagID < result[j].FlagID })
	return result, nil
}

func (m *MockStore) LinkFlagToExperiment(ctx context.Context, flagID, experimentID string) error {
	m.mu.Lock()
	defer m.mu.Unlock()

	f, ok := m.flags[flagID]
	if !ok {
		return fmt.Errorf("flag not found: %s", flagID)
	}
	f.PromotedExperimentID = experimentID
	f.PromotedAt = time.Now()
	f.UpdatedAt = time.Now()
	return nil
}

func (m *MockStore) GetFlagByExperiment(ctx context.Context, experimentID string) (*Flag, error) {
	m.mu.RLock()
	defer m.mu.RUnlock()

	for _, f := range m.flags {
		if f.PromotedExperimentID == experimentID {
			return copyFlag(f), nil
		}
	}
	return nil, fmt.Errorf("no flag found for experiment: %s", experimentID)
}

func (m *MockStore) GetFlagsByTargetingRule(ctx context.Context, targetingRuleID string) ([]*Flag, error) {
	m.mu.RLock()
	defer m.mu.RUnlock()

	var result []*Flag
	for _, f := range m.flags {
		if f.TargetingRuleID == targetingRuleID {
			result = append(result, copyFlag(f))
		}
	}
	sort.Slice(result, func(i, j int) bool { return result[i].Name < result[j].Name })
	return result, nil
}

func (m *MockStore) GetPromotedFlags(ctx context.Context) ([]*Flag, error) {
	m.mu.RLock()
	defer m.mu.RUnlock()

	var result []*Flag
	for _, f := range m.flags {
		if f.PromotedExperimentID != "" {
			result = append(result, copyFlag(f))
		}
	}
	sort.Slice(result, func(i, j int) bool { return result[i].PromotedAt.After(result[j].PromotedAt) })
	return result, nil
}

// SetUpdatedAt sets the updated_at timestamp for a flag (for testing staleness).
func (m *MockStore) SetUpdatedAt(flagID string, t time.Time) {
	m.mu.Lock()
	defer m.mu.Unlock()
	if f, ok := m.flags[flagID]; ok {
		f.UpdatedAt = t
	}
}

func copyFlag(f *Flag) *Flag {
	c := *f
	c.Variants = make([]FlagVariant, len(f.Variants))
	copy(c.Variants, f.Variants)
	return &c
}

func generateSalt() string {
	b := make([]byte, 16)
	rand.Read(b)
	return hex.EncodeToString(b)
}
