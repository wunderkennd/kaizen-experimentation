// Package config provides shared Pulumi configuration types for the Kaizen
// infrastructure. All agents import this package for environment-aware settings.
package config

import (
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi/config"
)

// Environment represents the deployment target (dev, staging, prod).
type Environment string

const (
	Dev     Environment = "dev"
	Staging Environment = "staging"
	Prod    Environment = "prod"
)

// KaizenConfig holds shared configuration read from Pulumi stack config.
type KaizenConfig struct {
	Env    Environment
	Region string
}

// LoadConfig reads the Pulumi stack configuration and returns a KaizenConfig.
func LoadConfig(ctx *pulumi.Context) *KaizenConfig {
	cfg := config.New(ctx, "kaizen")

	env := Environment(cfg.Get("environment"))
	if env == "" {
		env = Dev
	}

	region := cfg.Get("region")
	if region == "" {
		region = "us-east-1"
	}

	return &KaizenConfig{
		Env:    env,
		Region: region,
	}
}

// IsProd returns true if the environment is production.
func (c *KaizenConfig) IsProd() bool {
	return c.Env == Prod
}

// IsStaging returns true if the environment is staging.
func (c *KaizenConfig) IsStaging() bool {
	return c.Env == Staging
}
