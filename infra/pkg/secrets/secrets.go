// Package secrets provisions AWS Secrets Manager secrets for all Kaizen
// service credentials: database (PG), Kafka (SASL), Redis (AUTH), and
// OAuth2 client credentials. Secrets are structured as JSON for
// auto-rotation readiness.
package secrets

import (
	"encoding/json"

	"github.com/kennethsylvain/kaizen-experimentation/infra/pkg/config"
	"github.com/pulumi/pulumi-aws/sdk/v6/go/aws/secretsmanager"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"
)

// SecretsOutputs exports the ARNs of all provisioned secrets for
// consumption by downstream modules (compute, services).
type SecretsOutputs struct {
	DatabaseSecretArn pulumi.StringOutput
	KafkaSecretArn    pulumi.StringOutput
	RedisSecretArn    pulumi.StringOutput
	AuthSecretArn     pulumi.StringOutput
}

// DatabaseSecret holds the JSON-serializable structure for the PG
// connection secret. The shape follows the AWS Secrets Manager rotation
// template for RDS PostgreSQL.
type DatabaseSecret struct {
	Engine   string `json:"engine"`
	Host     string `json:"host"`
	Port     int    `json:"port"`
	Username string `json:"username"`
	Password string `json:"password"`
	Dbname   string `json:"dbname"`
}

// KafkaSecret holds SASL/SCRAM credentials and bootstrap servers for MSK.
type KafkaSecret struct {
	SaslUsername    string `json:"sasl_username"`
	SaslPassword   string `json:"sasl_password"`
	SaslMechanism  string `json:"sasl_mechanism"`
	BootstrapBrokers string `json:"bootstrap_brokers"`
}

// RedisSecret holds the AUTH token and endpoint for ElastiCache Redis.
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

// NewSecrets creates all four Secrets Manager secrets with environment-
// appropriate recovery windows.
func NewSecrets(ctx *pulumi.Context, cfg *config.Config) (*SecretsOutputs, error) {
	// recovery_window_in_days: 7 for prod (safety), 0 for dev/staging (instant cleanup).
	recoveryDays := 0
	if cfg.IsProd() {
		recoveryDays = 7
	}

	dbSecret, err := createSecret(ctx, cfg, "database", recoveryDays, DatabaseSecret{
		Engine:   "postgres",
		Host:     "placeholder-rds-endpoint",
		Port:     5432,
		Username: "kaizen",
		Password: "CHANGE_ME",
		Dbname:   "kaizen",
	})
	if err != nil {
		return nil, err
	}

	kafkaSecret, err := createSecret(ctx, cfg, "kafka", recoveryDays, KafkaSecret{
		SaslUsername:    "kaizen-msk-user",
		SaslPassword:   "CHANGE_ME",
		SaslMechanism:  "SCRAM-SHA-512",
		BootstrapBrokers: "placeholder-msk-brokers",
	})
	if err != nil {
		return nil, err
	}

	redisSecret, err := createSecret(ctx, cfg, "redis", recoveryDays, RedisSecret{
		AuthToken: "CHANGE_ME",
		Endpoint:  "placeholder-redis-endpoint",
		Port:      6379,
	})
	if err != nil {
		return nil, err
	}

	authSecret, err := createSecret(ctx, cfg, "auth", recoveryDays, AuthSecret{
		ClientID:     "kaizen-platform",
		ClientSecret: "CHANGE_ME",
		TokenURL:     "https://auth.example.com/oauth2/token",
		Issuer:       "https://auth.example.com",
	})
	if err != nil {
		return nil, err
	}

	return &SecretsOutputs{
		DatabaseSecretArn: dbSecret.Arn,
		KafkaSecretArn:    kafkaSecret.Arn,
		RedisSecretArn:    redisSecret.Arn,
		AuthSecretArn:     authSecret.Arn,
	}, nil
}

// createSecret provisions a single Secrets Manager secret with an initial
// placeholder version. The secret value is JSON-encoded from the provided
// struct. In Sprint I.1, the wiring task (I.1.4) replaces placeholders
// with actual resource outputs (RDS endpoint, MSK brokers, etc.).
func createSecret(
	ctx *pulumi.Context,
	cfg *config.Config,
	name string,
	recoveryDays int,
	value interface{},
) (*secretsmanager.Secret, error) {
	secretPath := cfg.SecretPath(name)
	resourceName := cfg.ResourceName("secret-" + name)

	secret, err := secretsmanager.NewSecret(ctx, resourceName, &secretsmanager.SecretArgs{
		Name:                    pulumi.String(secretPath),
		Description:             pulumi.Sprintf("Kaizen %s credentials (%s)", name, cfg.Env),
		RecoveryWindowInDays:    pulumi.Int(recoveryDays),
		ForceOverwriteReplicaSecret: pulumi.Bool(false),
		Tags: pulumi.StringMap{
			"Project":     pulumi.String(cfg.Project),
			"Environment": pulumi.String(string(cfg.Env)),
			"ManagedBy":   pulumi.String("pulumi"),
			"Component":   pulumi.String(name),
		},
	})
	if err != nil {
		return nil, err
	}

	jsonValue, err := json.Marshal(value)
	if err != nil {
		return nil, err
	}

	_, err = secretsmanager.NewSecretVersion(ctx, resourceName+"-version", &secretsmanager.SecretVersionArgs{
		SecretId:     secret.ID(),
		SecretString: pulumi.String(string(jsonValue)),
	})
	if err != nil {
		return nil, err
	}

	return secret, nil
}
