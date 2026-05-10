// Package secrets provisions GCP Secret Manager secrets for all Kaizen
// service credentials: database (PG), Kafka/Redpanda (SASL), Redis (AUTH),
// and OAuth2 client credentials. Secret payloads are JSON-serialized so
// the same wire shape works on AWS Secrets Manager and GCP Secret Manager.
//
// This module mirrors the public API of pkg/aws/secrets so that the
// per-cloud aggregator (pkg/gcp/gcp.go, added in a follow-up integration PR)
// can call NewSecrets identically on either provider.
//
// IAM bindings that grant compute service accounts read access to these
// secrets are intentionally NOT created here — on AWS they live in
// pkg/aws/compute/services.go (and pkg/aws/compute/migration.go), so we
// preserve that boundary for parity. Wiring will land alongside the GCP
// compute module (issue #486).
package secrets

import (
	"encoding/json"
	"fmt"

	"github.com/pulumi/pulumi-gcp/sdk/v8/go/gcp/secretmanager"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"

	"github.com/kaizen-experimentation/infra/pkg/config"
)

// SecretsOutputs exposes the resource-name paths of all provisioned secrets
// in their `projects/{project}/secrets/{secret_id}` form. Callers that need
// a `versions/latest` accessor should use the *Ref fields, which already
// append that suffix.
type SecretsOutputs struct {
	// DatabaseSecretName is the bare path: projects/<P>/secrets/<S>
	DatabaseSecretName pulumi.StringOutput
	KafkaSecretName    pulumi.StringOutput
	RedisSecretName    pulumi.StringOutput
	AuthSecretName     pulumi.StringOutput

	// DatabaseSecretRef is the version-qualified path:
	// projects/<P>/secrets/<S>/versions/latest. This is the form Cloud Run
	// expects when wiring secrets via env-var references and what AWS
	// compute consumers see as the cross-cloud SecretRef.
	DatabaseSecretRef pulumi.StringOutput
	KafkaSecretRef    pulumi.StringOutput
	RedisSecretRef    pulumi.StringOutput
	AuthSecretRef     pulumi.StringOutput
}

// DatabaseSecret holds the JSON-serializable structure for the Cloud SQL
// connection secret. The shape matches pkg/aws/secrets.DatabaseSecret so
// service code is identical on both clouds.
type DatabaseSecret struct {
	Engine   string `json:"engine"`
	Host     string `json:"host"`
	Port     int    `json:"port"`
	Username string `json:"username"`
	Password string `json:"password"`
	Dbname   string `json:"dbname"`
}

// KafkaSecret holds SASL/SCRAM credentials and bootstrap servers. Used
// against MSK on AWS and against Redpanda on GCP — same shape, same field
// names. Service code reads either transparently.
type KafkaSecret struct {
	SaslUsername     string `json:"sasl_username"`
	SaslPassword     string `json:"sasl_password"`
	SaslMechanism    string `json:"sasl_mechanism"`
	BootstrapBrokers string `json:"bootstrap_brokers"`
}

// RedisSecret holds the AUTH token and endpoint for Memorystore Redis.
type RedisSecret struct {
	AuthToken string `json:"auth_token"`
	Endpoint  string `json:"endpoint"`
	Port      int    `json:"port"`
}

// AuthSecret holds OAuth2 client credentials for the platform auth layer.
type AuthSecret struct {
	ClientID     string `json:"client_id"`
	ClientSecret string `json:"client_secret"`
	TokenURL     string `json:"token_url"`
	Issuer       string `json:"issuer"`
}

// SecretsInputs holds resource outputs from upstream Phase 1 modules
// (database, cache, streaming). These flow lazily through Pulumi outputs
// so the secrets module can be constructed before its dependencies have
// resolved their endpoints.
type SecretsInputs struct {
	// CloudSqlEndpoint is the Cloud SQL PostgreSQL primary endpoint
	// (host or host:port format).
	CloudSqlEndpoint pulumi.StringOutput
	// KafkaBootstrapBrokers is the comma-separated SASL bootstrap broker
	// list. On GCP this typically points at Redpanda; on AWS it points at
	// MSK. The shared SecretsInputs name avoids cloud-specific field names.
	KafkaBootstrapBrokers pulumi.StringOutput
	// RedisEndpoint is the Memorystore primary endpoint address.
	RedisEndpoint pulumi.StringOutput
}

// NewSecrets creates all four Secret Manager secrets (database, kafka,
// redis, auth) with automatic Google-managed replication. Resource
// endpoints from SecretsInputs feed the JSON payloads.
//
// Naming: the GCP SecretId must match [A-Za-z0-9_-]+, so we use
// cfg.ResourceName(...) (which produces `kaizen-<env>-<component>`) rather
// than cfg.SecretPath which embeds slashes for AWS-style hierarchical
// secrets.
func NewSecrets(ctx *pulumi.Context, cfg *config.Config, inputs *SecretsInputs) (*SecretsOutputs, error) {
	if inputs == nil {
		return nil, fmt.Errorf("gcp/secrets: SecretsInputs must not be nil")
	}

	dbSecret, err := newSecretContainer(ctx, cfg, "database")
	if err != nil {
		return nil, err
	}
	dbPayload := inputs.CloudSqlEndpoint.ApplyT(func(endpoint string) (string, error) {
		return marshalJSON(DatabaseSecret{
			Engine:   "postgres",
			Host:     endpoint,
			Port:     5432,
			Username: "kaizen_admin",
			Password: "CHANGE_ME",
			Dbname:   "kaizen",
		})
	}).(pulumi.StringOutput)
	if err := newSecretVersion(ctx, cfg, "database", dbSecret, dbPayload); err != nil {
		return nil, err
	}

	kafkaSecret, err := newSecretContainer(ctx, cfg, "kafka")
	if err != nil {
		return nil, err
	}
	kafkaPayload := inputs.KafkaBootstrapBrokers.ApplyT(func(brokers string) (string, error) {
		return marshalJSON(KafkaSecret{
			SaslUsername:     "kaizen-kafka-user",
			SaslPassword:     "CHANGE_ME",
			SaslMechanism:    "SCRAM-SHA-512",
			BootstrapBrokers: brokers,
		})
	}).(pulumi.StringOutput)
	if err := newSecretVersion(ctx, cfg, "kafka", kafkaSecret, kafkaPayload); err != nil {
		return nil, err
	}

	redisSecret, err := newSecretContainer(ctx, cfg, "redis")
	if err != nil {
		return nil, err
	}
	redisPayload := inputs.RedisEndpoint.ApplyT(func(endpoint string) (string, error) {
		return marshalJSON(RedisSecret{
			AuthToken: "CHANGE_ME",
			Endpoint:  endpoint,
			Port:      6379,
		})
	}).(pulumi.StringOutput)
	if err := newSecretVersion(ctx, cfg, "redis", redisSecret, redisPayload); err != nil {
		return nil, err
	}

	authSecret, err := newSecretContainer(ctx, cfg, "auth")
	if err != nil {
		return nil, err
	}
	authJSON, err := marshalJSON(AuthSecret{
		ClientID:     "kaizen-platform",
		ClientSecret: "CHANGE_ME",
		TokenURL:     "https://auth.example.com/oauth2/token",
		Issuer:       "https://auth.example.com",
	})
	if err != nil {
		return nil, err
	}
	if err := newSecretVersion(ctx, cfg, "auth", authSecret, pulumi.String(authJSON).ToStringOutput()); err != nil {
		return nil, err
	}

	return &SecretsOutputs{
		DatabaseSecretName: dbSecret.Name,
		KafkaSecretName:    kafkaSecret.Name,
		RedisSecretName:    redisSecret.Name,
		AuthSecretName:     authSecret.Name,
		DatabaseSecretRef:  versionLatestRef(dbSecret.Name),
		KafkaSecretRef:     versionLatestRef(kafkaSecret.Name),
		RedisSecretRef:     versionLatestRef(redisSecret.Name),
		AuthSecretRef:      versionLatestRef(authSecret.Name),
	}, nil
}

// newSecretContainer creates a Secret Manager secret with automatic
// Google-managed replication. The version is added separately so the
// caller can wire payloads from upstream pulumi.Outputs.
func newSecretContainer(ctx *pulumi.Context, cfg *config.Config, component string) (*secretmanager.Secret, error) {
	resourceName := cfg.ResourceName("secret-" + component)
	secret, err := secretmanager.NewSecret(ctx, resourceName, &secretmanager.SecretArgs{
		SecretId: pulumi.String(SecretID(cfg, component)),
		Replication: &secretmanager.SecretReplicationArgs{
			Auto: &secretmanager.SecretReplicationAutoArgs{},
		},
		Labels: pulumi.StringMap{
			"project":     pulumi.String(cfg.Project),
			"environment": pulumi.String(string(cfg.Env)),
			"managed_by":  pulumi.String("pulumi"),
			"component":   pulumi.String(component),
		},
	})
	if err != nil {
		return nil, fmt.Errorf("create %s secret: %w", component, err)
	}
	return secret, nil
}

// newSecretVersion attaches a payload version to an existing secret.
func newSecretVersion(
	ctx *pulumi.Context,
	cfg *config.Config,
	component string,
	secret *secretmanager.Secret,
	payload pulumi.StringOutput,
) error {
	resourceName := cfg.ResourceName("secret-" + component) + "-version"
	_, err := secretmanager.NewSecretVersion(ctx, resourceName, &secretmanager.SecretVersionArgs{
		Secret:     secret.Name,
		SecretData: payload,
	})
	if err != nil {
		return fmt.Errorf("create %s secret version: %w", component, err)
	}
	return nil
}

// SecretID derives the GCP SecretId for a given component. GCP requires
// SecretId to match [A-Za-z0-9_-]+, so we format as
// `kaizen-<env>-<component>` (which is what cfg.ResourceName already
// returns — exposed here as a pure function so tests don't need a Pulumi
// context).
func SecretID(cfg *config.Config, component string) string {
	return fmt.Sprintf("kaizen-%s-%s", cfg.Env, component)
}

// versionLatestRef appends "/versions/latest" to a secret resource-name
// output so callers get the form Cloud Run / GCE expects when wiring
// secrets via env-var references.
func versionLatestRef(name pulumi.StringOutput) pulumi.StringOutput {
	return pulumi.Sprintf("%s/versions/latest", name)
}

// marshalJSON is a tiny helper that wraps json.Marshal with a typed error
// so ApplyT callbacks can return the marshalled string + error tuple
// directly.
func marshalJSON(v interface{}) (string, error) {
	b, err := json.Marshal(v)
	if err != nil {
		return "", fmt.Errorf("marshal secret payload: %w", err)
	}
	return string(b), nil
}
