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
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"sync"
	"time"
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

// RemoteProvider calls the Assignment Service via JSON HTTP.
type RemoteProvider struct {
	baseURL   string
	timeoutMs int
	client    *http.Client
}

// NewRemoteProvider creates a provider that calls the Assignment Service.
func NewRemoteProvider(baseURL string) *RemoteProvider {
	return &RemoteProvider{baseURL: baseURL, timeoutMs: 2000}
}

func (p *RemoteProvider) Initialize(_ context.Context) error {
	p.client = &http.Client{
		Timeout: time.Duration(p.timeoutMs) * time.Millisecond,
	}
	return nil
}

// assignmentJSONRequest matches the server's JSON API request format.
type assignmentJSONRequest struct {
	UserID       string            `json:"userId"`
	ExperimentID string            `json:"experimentId,omitempty"`
	SessionID    string            `json:"sessionId,omitempty"`
	Attributes   map[string]string `json:"attributes,omitempty"`
}

// assignmentJSONResponse matches the server's JSON API response format.
type assignmentJSONResponse struct {
	ExperimentID          string  `json:"experimentId"`
	VariantID             string  `json:"variantId"`
	PayloadJSON           string  `json:"payloadJson"`
	AssignmentProbability float64 `json:"assignmentProbability"`
	IsActive              bool    `json:"isActive"`
}

// assignmentsJSONResponse wraps the bulk response.
type assignmentsJSONResponse struct {
	Assignments []assignmentJSONResponse `json:"assignments"`
}

func (p *RemoteProvider) GetAssignment(ctx context.Context, experimentID string, attrs UserAttributes) (*Assignment, error) {
	if p.client == nil {
		return nil, fmt.Errorf("provider not initialized")
	}

	url := p.baseURL + "/experimentation.assignment.v1.AssignmentService/GetAssignment"
	reqBody := assignmentJSONRequest{
		UserID:       attrs.UserID,
		ExperimentID: experimentID,
		Attributes:   flattenProps(attrs.Properties),
	}
	bodyBytes, err := json.Marshal(reqBody)
	if err != nil {
		return nil, fmt.Errorf("marshal request: %w", err)
	}

	req, err := http.NewRequestWithContext(ctx, http.MethodPost, url, bytes.NewReader(bodyBytes))
	if err != nil {
		return nil, fmt.Errorf("create request: %w", err)
	}
	req.Header.Set("Content-Type", "application/json")

	resp, err := p.client.Do(req)
	if err != nil {
		return nil, fmt.Errorf("http request: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return nil, nil
	}

	var data assignmentJSONResponse
	if err := json.NewDecoder(resp.Body).Decode(&data); err != nil {
		return nil, fmt.Errorf("decode response: %w", err)
	}

	if !data.IsActive || data.VariantID == "" {
		return nil, nil
	}

	var payload map[string]any
	if data.PayloadJSON != "" {
		if err := json.Unmarshal([]byte(data.PayloadJSON), &payload); err != nil {
			payload = nil
		}
	}

	return &Assignment{
		ExperimentID: data.ExperimentID,
		VariantName:  data.VariantID,
		Payload:      payload,
		FromCache:    false,
	}, nil
}

func (p *RemoteProvider) GetAllAssignments(ctx context.Context, attrs UserAttributes) (map[string]*Assignment, error) {
	if p.client == nil {
		return nil, fmt.Errorf("provider not initialized")
	}

	url := p.baseURL + "/experimentation.assignment.v1.AssignmentService/GetAssignments"
	reqBody := assignmentJSONRequest{
		UserID:     attrs.UserID,
		Attributes: flattenProps(attrs.Properties),
	}
	bodyBytes, err := json.Marshal(reqBody)
	if err != nil {
		return nil, fmt.Errorf("marshal request: %w", err)
	}

	req, err := http.NewRequestWithContext(ctx, http.MethodPost, url, bytes.NewReader(bodyBytes))
	if err != nil {
		return nil, fmt.Errorf("create request: %w", err)
	}
	req.Header.Set("Content-Type", "application/json")

	resp, err := p.client.Do(req)
	if err != nil {
		return nil, fmt.Errorf("http request: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return nil, nil
	}

	var data assignmentsJSONResponse
	if err := json.NewDecoder(resp.Body).Decode(&data); err != nil {
		return nil, fmt.Errorf("decode response: %w", err)
	}

	results := make(map[string]*Assignment, len(data.Assignments))
	for _, a := range data.Assignments {
		if !a.IsActive || a.VariantID == "" {
			continue
		}
		var payload map[string]any
		if a.PayloadJSON != "" {
			if err := json.Unmarshal([]byte(a.PayloadJSON), &payload); err != nil {
				payload = nil
			}
		}
		results[a.ExperimentID] = &Assignment{
			ExperimentID: a.ExperimentID,
			VariantName:  a.VariantID,
			Payload:      payload,
			FromCache:    false,
		}
	}
	return results, nil
}

func (p *RemoteProvider) Close() error {
	if p.client != nil {
		p.client.CloseIdleConnections()
	}
	return nil
}

// flattenProps converts map[string]any to map[string]string for the proto attributes.
func flattenProps(props map[string]any) map[string]string {
	if len(props) == 0 {
		return nil
	}
	result := make(map[string]string, len(props))
	for k, v := range props {
		result[k] = fmt.Sprint(v)
	}
	return result
}

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
	if len(config.Variants) == 0 {
		return nil, nil
	}

	bucket := computeBucket(attrs.UserID, config.HashSalt, uint32(config.TotalBuckets))

	if !isInAllocation(bucket, uint32(config.AllocationStart), uint32(config.AllocationEnd)) {
		return nil, nil
	}

	allocSize := float64(config.AllocationEnd - config.AllocationStart + 1)
	relativeBucket := float64(bucket - uint32(config.AllocationStart))

	cumulative := 0.0
	for _, v := range config.Variants {
		cumulative += v.TrafficFraction * allocSize
		if relativeBucket < cumulative {
			return &Assignment{
				ExperimentID: config.ExperimentID,
				VariantName:  v.Name,
				Payload:      v.Payload,
				FromCache:    true,
			}, nil
		}
	}

	// FP rounding fallback — assign to last variant
	last := config.Variants[len(config.Variants)-1]
	return &Assignment{
		ExperimentID: config.ExperimentID,
		VariantName:  last.Name,
		Payload:      last.Payload,
		FromCache:    true,
	}, nil
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
