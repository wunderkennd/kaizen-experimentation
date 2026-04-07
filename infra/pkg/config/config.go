// Package config provides shared configuration types and helpers for the
// Kaizen IaC modules. All agents import this package; changes require
// cross-agent coordination.
package config

import (
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"
	pulumiconfig "github.com/pulumi/pulumi/sdk/v3/go/pulumi/config"
)

// Env reads the "env" config key (dev | staging | prod).
func Env(ctx *pulumi.Context) string {
	c := pulumiconfig.New(ctx, "kaizen")
	return c.Require("env")
}

// DefaultTags returns the base tags applied to every resource.
func DefaultTags(env string) pulumi.StringMap {
	return pulumi.StringMap{
		"Project":     pulumi.String("kaizen"),
		"Environment": pulumi.String(env),
		"ManagedBy":   pulumi.String("pulumi"),
	}
}

// MergeTags merges extra tags into the default tag set.
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
