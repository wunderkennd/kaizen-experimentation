// Package types — provider-agnostic configuration helpers. See outputs.go for
// the shared output struct contract.
package types

// CloudProvider identifies which cloud the stack targets.
type CloudProvider string

const (
	// CloudAWS selects the AWS module set in pkg/aws/.
	CloudAWS CloudProvider = "aws"
	// CloudGCP selects the GCP module set in pkg/gcp/ (Phase 1+).
	CloudGCP CloudProvider = "gcp"
)

// StreamingProvider identifies which Kafka-protocol service backs the
// streaming stage. Decoupled from CloudProvider so an AWS tenant can opt
// into Redpanda (or vice versa) without changing cloud provider.
type StreamingProvider string

const (
	// StreamingMSK selects AWS MSK (managed Kafka).
	StreamingMSK StreamingProvider = "msk"
	// StreamingRedpanda selects Redpanda Cloud (Phase 2+).
	StreamingRedpanda StreamingProvider = "redpanda"
)

// TenantConfig holds the provider-agnostic fields that Deploy() needs to
// dispatch via switch. Concrete config types in pkg/config/ embed or expose
// these fields so Deploy() can read them generically.
type TenantConfig struct {
	// CloudProvider — "aws" (default) or "gcp".
	CloudProvider CloudProvider
	// StreamingProvider — "msk" (default) or "redpanda".
	StreamingProvider StreamingProvider
}
