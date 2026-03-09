package store

import (
	"context"
	"fmt"
	"math/rand"
	"sync"
	"sync/atomic"
	"time"
)

// FailureMode controls how a ChaosStore method fails.
type FailureMode int

const (
	FailNone    FailureMode = iota // No failure injection.
	FailAlways                     // Every call fails.
	FailAfterN                     // Fail after N successful calls.
	FailRandom                     // Fail with given probability per call.
)

// ChaosConfig configures failure injection for a single store method.
type ChaosConfig struct {
	Mode        FailureMode
	Err         error   // Error to return. Defaults to errChaosDefault.
	AfterN      int     // For FailAfterN: succeed first N calls, then fail.
	Probability float64 // For FailRandom: probability [0,1] of failure per call.
}

var errChaosDefault = fmt.Errorf("chaos: simulated store failure")

// ChaosStore wraps a Store and injects configurable failures per-method.
// Thread-safe for use with -race.
type ChaosStore struct {
	inner    Store
	mu       sync.RWMutex
	configs  map[string]ChaosConfig
	counters map[string]*atomic.Int64
	rng      *rand.Rand
	rngMu    sync.Mutex
}

// NewChaosStore wraps the given store with chaos injection capabilities.
func NewChaosStore(inner Store) *ChaosStore {
	return &ChaosStore{
		inner:    inner,
		configs:  make(map[string]ChaosConfig),
		counters: make(map[string]*atomic.Int64),
		rng:      rand.New(rand.NewSource(time.Now().UnixNano())),
	}
}

// SetFailure configures failure injection for a named method.
func (c *ChaosStore) SetFailure(method string, cfg ChaosConfig) {
	c.mu.Lock()
	defer c.mu.Unlock()
	c.configs[method] = cfg
	if _, ok := c.counters[method]; !ok {
		c.counters[method] = &atomic.Int64{}
	}
}

// ClearAllFailures removes all failure injection configs.
func (c *ChaosStore) ClearAllFailures() {
	c.mu.Lock()
	defer c.mu.Unlock()
	c.configs = make(map[string]ChaosConfig)
}

// CallCount returns the number of times a method has been invoked.
func (c *ChaosStore) CallCount(method string) int64 {
	c.mu.RLock()
	counter, ok := c.counters[method]
	c.mu.RUnlock()
	if !ok {
		return 0
	}
	return counter.Load()
}

// shouldFail checks if the given method should fail on this call.
func (c *ChaosStore) shouldFail(method string) error {
	c.mu.RLock()
	cfg, ok := c.configs[method]
	counter := c.counters[method]
	c.mu.RUnlock()

	if !ok {
		return nil
	}

	if counter != nil {
		counter.Add(1)
	}

	errToReturn := cfg.Err
	if errToReturn == nil {
		errToReturn = errChaosDefault
	}

	switch cfg.Mode {
	case FailNone:
		return nil
	case FailAlways:
		return errToReturn
	case FailAfterN:
		if counter != nil && counter.Load() > int64(cfg.AfterN) {
			return errToReturn
		}
		return nil
	case FailRandom:
		c.rngMu.Lock()
		r := c.rng.Float64()
		c.rngMu.Unlock()
		if r < cfg.Probability {
			return errToReturn
		}
		return nil
	default:
		return nil
	}
}

// Inner returns the underlying store (for test assertions).
func (c *ChaosStore) Inner() Store {
	return c.inner
}

func (c *ChaosStore) CreateFlag(ctx context.Context, f *Flag) (*Flag, error) {
	if err := c.shouldFail("CreateFlag"); err != nil {
		return nil, err
	}
	return c.inner.CreateFlag(ctx, f)
}

func (c *ChaosStore) GetFlag(ctx context.Context, flagID string) (*Flag, error) {
	if err := c.shouldFail("GetFlag"); err != nil {
		return nil, err
	}
	return c.inner.GetFlag(ctx, flagID)
}

func (c *ChaosStore) UpdateFlag(ctx context.Context, f *Flag) (*Flag, error) {
	if err := c.shouldFail("UpdateFlag"); err != nil {
		return nil, err
	}
	return c.inner.UpdateFlag(ctx, f)
}

func (c *ChaosStore) DeleteFlag(ctx context.Context, flagID string) error {
	if err := c.shouldFail("DeleteFlag"); err != nil {
		return err
	}
	return c.inner.DeleteFlag(ctx, flagID)
}

func (c *ChaosStore) ListFlags(ctx context.Context, pageSize int, pageToken string) ([]*Flag, string, error) {
	if err := c.shouldFail("ListFlags"); err != nil {
		return nil, "", err
	}
	return c.inner.ListFlags(ctx, pageSize, pageToken)
}

func (c *ChaosStore) GetAllEnabledFlags(ctx context.Context) ([]*Flag, error) {
	if err := c.shouldFail("GetAllEnabledFlags"); err != nil {
		return nil, err
	}
	return c.inner.GetAllEnabledFlags(ctx)
}

func (c *ChaosStore) LinkFlagToExperiment(ctx context.Context, flagID, experimentID string) error {
	if err := c.shouldFail("LinkFlagToExperiment"); err != nil {
		return err
	}
	return c.inner.LinkFlagToExperiment(ctx, flagID, experimentID)
}

func (c *ChaosStore) GetFlagByExperiment(ctx context.Context, experimentID string) (*Flag, error) {
	if err := c.shouldFail("GetFlagByExperiment"); err != nil {
		return nil, err
	}
	return c.inner.GetFlagByExperiment(ctx, experimentID)
}

func (c *ChaosStore) GetFlagsByTargetingRule(ctx context.Context, targetingRuleID string) ([]*Flag, error) {
	if err := c.shouldFail("GetFlagsByTargetingRule"); err != nil {
		return nil, err
	}
	return c.inner.GetFlagsByTargetingRule(ctx, targetingRuleID)
}

func (c *ChaosStore) GetPromotedFlags(ctx context.Context) ([]*Flag, error) {
	if err := c.shouldFail("GetPromotedFlags"); err != nil {
		return nil, err
	}
	return c.inner.GetPromotedFlags(ctx)
}

// ChaosAuditStore wraps an AuditStore and injects failures on RecordAudit.
// Read methods pass through unchanged.
type ChaosAuditStore struct {
	inner   AuditStore
	mu      sync.RWMutex
	failErr error
}

// NewChaosAuditStore wraps the given audit store with chaos injection.
func NewChaosAuditStore(inner AuditStore) *ChaosAuditStore {
	return &ChaosAuditStore{inner: inner}
}

// SetFailAll configures RecordAudit to always return the given error.
func (c *ChaosAuditStore) SetFailAll(err error) {
	c.mu.Lock()
	defer c.mu.Unlock()
	c.failErr = err
}

// ClearFailure removes the failure injection.
func (c *ChaosAuditStore) ClearFailure() {
	c.mu.Lock()
	defer c.mu.Unlock()
	c.failErr = nil
}

func (c *ChaosAuditStore) RecordAudit(ctx context.Context, entry *AuditEntry) error {
	c.mu.RLock()
	err := c.failErr
	c.mu.RUnlock()
	if err != nil {
		return err
	}
	return c.inner.RecordAudit(ctx, entry)
}

func (c *ChaosAuditStore) GetFlagAuditLog(ctx context.Context, flagID string, limit int) ([]*AuditEntry, error) {
	return c.inner.GetFlagAuditLog(ctx, flagID, limit)
}

func (c *ChaosAuditStore) GetStaleFlags(ctx context.Context, staleThreshold time.Duration) ([]*StaleFlagEntry, error) {
	return c.inner.GetStaleFlags(ctx, staleThreshold)
}
