// Package config provides shared Pulumi configuration types for all infra modules.
// Owner: Infra-2 (I.0.1). Other agents add minimal stubs here until the full
// config package lands.
package config

import "github.com/pulumi/pulumi/sdk/v3/go/pulumi"

// KaizenConfig holds stack-level settings read from Pulumi.<stack>.yaml.
type KaizenConfig struct {
	Environment string // "dev", "staging", "prod"
	Domain      string // Base domain, e.g. "example.com"
	ProjectName string // "kaizen"
}

// ALBOutputs are produced by the ALB module and consumed by DNS and compute modules.
type ALBOutputs struct {
	ALBArn          pulumi.StringOutput
	ALBDNSName      pulumi.StringOutput
	ALBHostedZoneID pulumi.StringOutput
	ListenerArn     pulumi.StringOutput
}

// DNSOutputs are produced by the DNS module and consumed by downstream modules.
type DNSOutputs struct {
	HostedZoneID   pulumi.IDOutput
	CertificateArn pulumi.StringOutput
}
