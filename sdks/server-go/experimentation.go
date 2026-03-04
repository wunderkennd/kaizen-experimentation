// Package experimentation provides a Go SDK for the Experimentation Platform.
//
// It implements the Provider Abstraction pattern (ADR-007) with three backends:
//   - RemoteProvider: Calls the Assignment Service via ConnectRPC
//   - LocalProvider:  Evaluates assignments locally using cached config + CGo hash
//   - MockProvider:   Returns deterministic assignments for testing
//
// Usage:
//
//	client, _ := experimentation.NewClient(experimentation.Config{
//	    Provider: experimentation.NewRemoteProvider("https://assignment.example.com"),
//	})
//	defer client.Close()
//
//	variant, _ := client.GetVariant(ctx, "homepage_recs_v2", "user-123", nil)
package experimentation

import (
	"context"
	"sync"
)

// ---------------------------------------------------------------------------
// Core Types
// ---------------------------------------------------------------------------

// Assignment represents a variant assignment for a single experiment.
type Assignment struct {
	ExperimentID string
	VariantName  string
	Payload      map[string]any
	FromCache    bool
}

// UserAttributes holds attributes for targeting evaluation.
type UserAttributes struct {
	UserID     string
	Properties map[string]any
}

// ---------------------------------------------------------------------------
// Provider Interface
// ---------------------------------------------------------------------------

// AssignmentProvider is the interface all assignment backends must implement.
// See ADR-007 for the design rationale.
type AssignmentProvider interface {
	// Initialize prepares the provider (establish connections, fetch config).
	Initialize(ctx context.Context) error

	// GetAssignment returns a variant for the given experiment and user.
	// Returns nil if the user is not in the experiment.
	GetAssignment(ctx context.Context, experimentID string, attrs UserAttributes) (*Assignment, error)

	// GetAllAssignments returns assignments for all active experiments.
	GetAllAssignments(ctx context.Context, attrs UserAttributes) (map[string]*Assignment, error)

	// Close shuts down the provider and releases resources.
	Close() error
}

// ---------------------------------------------------------------------------
// RemoteProvider
// ---------------------------------------------------------------------------

// RemoteProvider calls the Assignment Service via ConnectRPC.
type RemoteProvider struct {
	baseURL   string
	timeoutMs int
}

// NewRemoteProvider creates a provider that calls the Assignment Service.
func NewRemoteProvider(baseURL string) *RemoteProvider {
	return &RemoteProvider{baseURL: baseURL, timeoutMs: 2000}
}

func (p *RemoteProvider) Initialize(_ context.Context) error {
	// TODO (Agent-1): Create ConnectRPC client for AssignmentService
	return nil
}

func (p *RemoteProvider) GetAssignment(_ context.Context, experimentID string, attrs UserAttributes) (*Assignment, error) {
	// TODO (Agent-1): Call AssignmentService.GetAssignment
	_ = experimentID
	_ = attrs
	return nil, nil
}

func (p *RemoteProvider) GetAllAssignments(_ context.Context, attrs UserAttributes) (map[string]*Assignment, error) {
	// TODO (Agent-1): Call AssignmentService.GetAllAssignments
	_ = attrs
	return nil, nil
}

func (p *RemoteProvider) Close() error { return nil }

// ---------------------------------------------------------------------------
// LocalProvider
// ---------------------------------------------------------------------------

// ExperimentConfig holds the config for local assignment evaluation.
type ExperimentConfig struct {
	ExperimentID    string
	HashSalt        string
	LayerName       string
	Variants        []VariantConfig
	AllocationStart int
	AllocationEnd   int
	TotalBuckets    int
}

// VariantConfig holds variant-level configuration.
type VariantConfig struct {
	Name            string
	TrafficFraction float64
	IsControl       bool
	Payload         map[string]any
}

// LocalProvider evaluates assignments locally using cached config.
type LocalProvider struct {
	experiments map[string]ExperimentConfig
}

// NewLocalProvider creates a provider for local assignment evaluation.
func NewLocalProvider(configs []ExperimentConfig) *LocalProvider {
	m := make(map[string]ExperimentConfig, len(configs))
	for _, c := range configs {
		m[c.ExperimentID] = c
	}
	return &LocalProvider{experiments: m}
}

func (p *LocalProvider) Initialize(_ context.Context) error { return nil }

func (p *LocalProvider) GetAssignment(_ context.Context, experimentID string, attrs UserAttributes) (*Assignment, error) {
	config, ok := p.experiments[experimentID]
	if !ok {
		return nil, nil
	}

	// TODO (Agent-1): Use CGo binding to experimentation_bucket() from experimentation-ffi
	//   1. bucket = experimentation_bucket(attrs.UserID, config.HashSalt, config.TotalBuckets)
	//   2. if bucket < config.AllocationStart || bucket > config.AllocationEnd → nil
	//   3. Map bucket to variant by cumulative traffic fractions
	_ = config
	_ = attrs
	return nil, nil
}

func (p *LocalProvider) GetAllAssignments(ctx context.Context, attrs UserAttributes) (map[string]*Assignment, error) {
	results := make(map[string]*Assignment, len(p.experiments))
	for id := range p.experiments {
		a, err := p.GetAssignment(ctx, id, attrs)
		if err != nil {
			return nil, err
		}
		if a != nil {
			results[id] = a
		}
	}
	return results, nil
}

func (p *LocalProvider) Close() error { return nil }

// ---------------------------------------------------------------------------
// MockProvider
// ---------------------------------------------------------------------------

// MockProvider returns deterministic assignments for testing.
type MockProvider struct {
	mu          sync.RWMutex
	assignments map[string]*Assignment
}

// NewMockProvider creates a mock provider with predefined assignments.
func NewMockProvider(assignments map[string]*Assignment) *MockProvider {
	if assignments == nil {
		assignments = make(map[string]*Assignment)
	}
	return &MockProvider{assignments: assignments}
}

func (p *MockProvider) Initialize(_ context.Context) error { return nil }

func (p *MockProvider) GetAssignment(_ context.Context, experimentID string, _ UserAttributes) (*Assignment, error) {
	p.mu.RLock()
	defer p.mu.RUnlock()
	return p.assignments[experimentID], nil
}

func (p *MockProvider) GetAllAssignments(_ context.Context, _ UserAttributes) (map[string]*Assignment, error) {
	p.mu.RLock()
	defer p.mu.RUnlock()
	results := make(map[string]*Assignment, len(p.assignments))
	for k, v := range p.assignments {
		results[k] = v
	}
	return results, nil
}

// SetAssignment overrides an assignment at runtime (useful in tests).
func (p *MockProvider) SetAssignment(experimentID, variantName string) {
	p.mu.Lock()
	defer p.mu.Unlock()
	p.assignments[experimentID] = &Assignment{
		ExperimentID: experimentID,
		VariantName:  variantName,
	}
}

func (p *MockProvider) Close() error { return nil }

// ---------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------

// Config holds configuration for the ExperimentClient.
type Config struct {
	Provider         AssignmentProvider
	FallbackProvider AssignmentProvider // optional — ADR-007 fallback chain
}

// Client is the main entry point for the SDK.
type Client struct {
	provider AssignmentProvider
	fallback AssignmentProvider
}

// NewClient creates a new experiment client and initializes the provider(s).
func NewClient(ctx context.Context, cfg Config) (*Client, error) {
	if err := cfg.Provider.Initialize(ctx); err != nil {
		return nil, err
	}
	if cfg.FallbackProvider != nil {
		if err := cfg.FallbackProvider.Initialize(ctx); err != nil {
			return nil, err
		}
	}
	return &Client{provider: cfg.Provider, fallback: cfg.FallbackProvider}, nil
}

// GetVariant returns the variant name for the given experiment, or "" if not assigned.
func (c *Client) GetVariant(ctx context.Context, experimentID, userID string, props map[string]any) (string, error) {
	a, err := c.GetAssignment(ctx, experimentID, userID, props)
	if err != nil {
		return "", err
	}
	if a == nil {
		return "", nil
	}
	return a.VariantName, nil
}

// GetAssignment returns the full assignment for the given experiment.
func (c *Client) GetAssignment(ctx context.Context, experimentID, userID string, props map[string]any) (*Assignment, error) {
	attrs := UserAttributes{UserID: userID, Properties: props}

	a, err := c.provider.GetAssignment(ctx, experimentID, attrs)
	if err != nil && c.fallback != nil {
		return c.fallback.GetAssignment(ctx, experimentID, attrs)
	}
	return a, err
}

// Close shuts down the client and releases all resources.
func (c *Client) Close() error {
	err := c.provider.Close()
	if c.fallback != nil {
		if fbErr := c.fallback.Close(); fbErr != nil && err == nil {
			err = fbErr
		}
	}
	return err
}
