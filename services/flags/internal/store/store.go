package store

import (
	"context"
	"time"
)

// Flag is the domain model for a feature flag.
type Flag struct {
	FlagID            string
	Name              string
	Description       string
	Type              string // BOOLEAN, STRING, NUMERIC, JSON
	DefaultValue      string
	Enabled           bool
	RolloutPercentage float64
	Salt              string
	TargetingRuleID   string
	Variants          []FlagVariant
	CreatedAt         time.Time
	UpdatedAt         time.Time

	// Flag-experiment linkage (Phase 3): set when PromoteToExperiment succeeds.
	PromotedExperimentID string
	PromotedAt           time.Time
	// ResolvedAt tracks when a promoted flag was resolved (auto or manual).
	// Zero value means not yet resolved.
	ResolvedAt time.Time
}

// FlagVariant is a variant within a feature flag.
type FlagVariant struct {
	VariantID       string
	FlagID          string
	Value           string
	TrafficFraction float64
	Ordinal         int
}

// Store defines the persistence interface for feature flags.
type Store interface {
	CreateFlag(ctx context.Context, f *Flag) (*Flag, error)
	GetFlag(ctx context.Context, flagID string) (*Flag, error)
	UpdateFlag(ctx context.Context, f *Flag) (*Flag, error)
	DeleteFlag(ctx context.Context, flagID string) error
	ListFlags(ctx context.Context, pageSize int, pageToken string) ([]*Flag, string, error)
	GetAllEnabledFlags(ctx context.Context) ([]*Flag, error)

	// Flag-experiment linkage: record which experiment a flag was promoted to.
	LinkFlagToExperiment(ctx context.Context, flagID, experimentID string) error
	// GetFlagByExperiment returns the flag that was promoted to a given experiment.
	GetFlagByExperiment(ctx context.Context, experimentID string) (*Flag, error)

	// Dependency tracking: find all flags using a targeting rule.
	GetFlagsByTargetingRule(ctx context.Context, targetingRuleID string) ([]*Flag, error)
	// GetPromotedFlags returns all flags that have been promoted to experiments.
	GetPromotedFlags(ctx context.Context) ([]*Flag, error)
}
