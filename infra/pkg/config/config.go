// Package config provides shared configuration types and helpers for the
// Kaizen infrastructure modules. All modules read Pulumi config through
// these helpers to ensure consistent defaults and tagging.
package config

import (
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi/config"
)

// CommonTags returns the base tag set that every AWS resource must carry.
func CommonTags(ctx *pulumi.Context) pulumi.StringMap {
	cfg := config.New(ctx, "kaizen")
	env := cfg.Get("environment")
	if env == "" {
		env = "dev"
	}
	return pulumi.StringMap{
		"Environment": pulumi.String(env),
		"Project":     pulumi.String("kaizen"),
		"ManagedBy":   pulumi.String("pulumi"),
	}
}

// MergeTags merges additional tags into the common tag set.
func MergeTags(base pulumi.StringMap, extra pulumi.StringMap) pulumi.StringMap {
	merged := pulumi.StringMap{}
	for k, v := range base {
		merged[k] = v
	}
	for k, v := range extra {
		merged[k] = v
	}
	return merged
}
