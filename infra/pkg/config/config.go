// Package config provides shared configuration types and helpers for all
// infrastructure modules. Every module imports this package to read
// environment-specific Pulumi stack config values.
package config

import (
	"fmt"

	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi/config"
)

// Environment represents a deployment target (dev, staging, prod).
type Environment string

const (
	Dev     Environment = "dev"
	Staging Environment = "staging"
	Prod    Environment = "prod"
)

// Config holds the shared configuration read from Pulumi stack config.
type Config struct {
	Env       Environment
	Project   string
	AwsRegion string
}

// LoadConfig reads environment-specific values from the Pulumi stack config.
func LoadConfig(ctx *pulumi.Context) (*Config, error) {
	cfg := config.New(ctx, "kaizen")

	env := Environment(cfg.Require("environment"))
	switch env {
	case Dev, Staging, Prod:
	default:
		return nil, fmt.Errorf("invalid environment %q: must be dev, staging, or prod", env)
	}

	awsCfg := config.New(ctx, "aws")

	return &Config{
		Env:       env,
		Project:   "kaizen",
		AwsRegion: awsCfg.Require("region"),
	}, nil
}

// ResourceName returns a prefixed resource name like "kaizen-dev-rds".
func (c *Config) ResourceName(component string) string {
	return fmt.Sprintf("%s-%s-%s", c.Project, c.Env, component)
}

// SecretPath returns a Secrets Manager path like "kaizen/dev/database".
func (c *Config) SecretPath(name string) string {
	return fmt.Sprintf("%s/%s/%s", c.Project, c.Env, name)
}

// IsProd returns true if the environment is production.
func (c *Config) IsProd() bool {
	return c.Env == Prod
}
