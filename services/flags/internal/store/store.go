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
}
