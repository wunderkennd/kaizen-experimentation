// Package types defines provider-agnostic output structs that AWS and GCP
// modules return and that Deploy() composes via switch-dispatch. The shapes
// here are the contract between Deploy() and provider implementations: any
// AWS or GCP module that fulfills a stage MUST return one of these structs.
//
// Where a field's interpretation differs by provider (e.g., AWS ARN vs GCP
// resource name), the field name uses a generic suffix like "Ref" and the
// doc comment explains both interpretations.
package types

import "github.com/pulumi/pulumi/sdk/v3/go/pulumi"

// NetworkOutputs holds VPC, subnets, security groups, and service discovery
// outputs produced by the network stage. All downstream stages depend on this.
type NetworkOutputs struct {
	// VpcId is the cloud-native VPC identifier (AWS VPC ID, GCP network self-link).
	VpcId pulumi.IDOutput

	// PublicSubnetIds lists subnets that hold internet-facing resources
	// (load balancers, NAT). Three AZs in AWS, three zones in GCP.
	PublicSubnetIds pulumi.StringArrayOutput

	// PrivateSubnetIds lists subnets that hold private resources
	// (databases, compute, caches). Three AZs in AWS, three zones in GCP.
	PrivateSubnetIds pulumi.StringArrayOutput

	// SecurityGroupIds maps role keys to cloud-native security group identifiers.
	// Standard keys: "ecs", "alb", "rds", "redis", "msk", "m4b", "schema-registry".
	// In GCP, values represent firewall rule self-links keyed the same way.
	SecurityGroupIds map[string]pulumi.IDOutput

	// ServiceDiscoveryId is the AWS Cloud Map namespace ID, or in GCP the
	// Service Directory namespace resource name.
	ServiceDiscoveryId pulumi.IDOutput

	// PrivateRouteTableIds is AWS-specific (private route tables consumed by
	// VPC endpoints). On GCP this is left zero-valued — GCP networks use
	// implicit routing.
	PrivateRouteTableIds pulumi.StringArrayOutput

	// S3VpcEndpointId is AWS-specific (gateway VPC endpoint ID consumed by
	// the S3 bucket policy's aws:sourceVpce condition). Zero-valued on GCP.
	// Kept as IDOutput because the storage module's bucket-policy ApplyT chain
	// type-asserts the underlying value as pulumi.ID, not string.
	S3VpcEndpointId pulumi.IDOutput
}

// DatabaseOutputs holds outputs from the relational database stage
// (AWS RDS PostgreSQL, GCP Cloud SQL PostgreSQL).
type DatabaseOutputs struct {
	// Endpoint is host:port form, suitable for direct connection strings.
	Endpoint pulumi.StringOutput

	// Port is the listener port (5432 for PostgreSQL).
	Port pulumi.IntOutput

	// InstanceId is the cloud-native instance identifier used for alarms
	// and operational tooling (RDS instance identifier on AWS, Cloud SQL
	// instance name on GCP).
	InstanceId pulumi.StringOutput
}

// CacheOutputs holds outputs from the in-memory cache stage
// (AWS ElastiCache Redis, GCP Memorystore Redis).
type CacheOutputs struct {
	// Endpoint is the primary connection endpoint
	// (e.g. "redis://host:6379" or just "host:6379" depending on caller).
	Endpoint pulumi.StringOutput
}

// StorageOutputs holds outputs from the object storage stage
// (AWS S3 buckets, GCP Cloud Storage buckets).
type StorageOutputs struct {
	// DataBucketName is the bare bucket name (no scheme).
	DataBucketName pulumi.StringOutput
	// DataBucketRef is the cloud-native reference: ARN on AWS, gs:// URI on GCP.
	DataBucketRef pulumi.StringOutput

	// MlflowBucketName / MlflowBucketRef — same shape, for MLflow artifacts.
	MlflowBucketName pulumi.StringOutput
	MlflowBucketRef  pulumi.StringOutput

	// LogsBucketName / LogsBucketRef — same shape, for ALB / load-balancer logs.
	LogsBucketName pulumi.StringOutput
	LogsBucketRef  pulumi.StringOutput
}

// IAMOutputs holds outputs from the identity stage. References are cloud-
// native: ARNs on AWS, service-account emails or principal names on GCP.
type IAMOutputs struct {
	// ExecRoleRef is the role/identity that grants permission to *launch*
	// containers (ECS task execution role on AWS; Cloud Run service runtime
	// service account on GCP).
	ExecRoleRef pulumi.StringOutput

	// TaskRoleRef is the role/identity that the running container assumes
	// for accessing AWS/GCP services from application code.
	TaskRoleRef pulumi.StringOutput
}

// StreamingOutputs holds outputs from the Kafka-protocol streaming stage
// (AWS MSK or Redpanda Cloud).
type StreamingOutputs struct {
	// BootstrapBrokers is the comma-separated bootstrap server list.
	BootstrapBrokers pulumi.StringOutput

	// SchemaRegistryUrl is the URL of the Confluent-compatible schema registry
	// (Schema Registry service on ECS/Cloud Run, or Redpanda's built-in registry).
	SchemaRegistryUrl pulumi.StringOutput

	// ClusterArn is the AWS MSK cluster ARN. Empty for Redpanda.
	ClusterArn pulumi.StringOutput

	// ClusterName is a human-readable cluster identifier used by alarms.
	ClusterName pulumi.StringOutput
}

// ComputeOutputs holds outputs from the compute stage
// (ECS Fargate + EC2 on AWS, Cloud Run + GCE on GCP).
type ComputeOutputs struct {
	// ClusterId identifies the orchestration cluster
	// (ECS cluster ID on AWS, GKE cluster ID or "" on GCP if using Cloud Run only).
	ClusterId pulumi.StringOutput

	// ClusterName is the human-readable cluster name (used by alarms).
	ClusterName pulumi.StringOutput

	// ClusterArn is the AWS ECS cluster ARN. Empty on GCP.
	ClusterArn pulumi.StringOutput

	// ServiceEndpoints maps service name (e.g., "m1-assignment", "m7-flags") to
	// the internal service URL (Cloud Map FQDN on AWS, Service Directory or
	// Cloud Run URL on GCP).
	ServiceEndpoints map[string]pulumi.StringOutput

	// ServiceArns maps service name to the cloud-native service identifier
	// (ECS service ARN on AWS, Cloud Run service ID on GCP).
	ServiceArns map[string]pulumi.StringOutput

	// M4bInstanceId is the dedicated stateful instance identifier
	// (EC2 instance ID via ASG on AWS, GCE instance ID via MIG on GCP).
	M4bInstanceId pulumi.StringOutput

	// M4bEndpoint is the resolvable internal endpoint for M4b.
	M4bEndpoint pulumi.StringOutput

	// M4bAsgName is the AWS Auto Scaling Group name backing M4b.
	// On GCP this would be a Managed Instance Group name. Used by CloudWatch
	// alarms today.
	M4bAsgName pulumi.StringOutput
}

// SecretsOutputs holds references to secrets in the cloud-native secret store
// (AWS Secrets Manager, GCP Secret Manager).
type SecretsOutputs struct {
	// DatabaseSecretRef points at the DB credentials secret. Format is the
	// native reference: ARN on AWS, "projects/<P>/secrets/<S>" path on GCP.
	DatabaseSecretRef pulumi.StringOutput

	// KafkaSecretRef points at the SCRAM/SASL credentials secret used by
	// MSK (AWS) or by the Redpanda client.
	KafkaSecretRef pulumi.StringOutput

	// RedisSecretRef points at the Redis auth-token secret.
	RedisSecretRef pulumi.StringOutput

	// AuthSecretRef points at the application JWT signing secret.
	AuthSecretRef pulumi.StringOutput
}

// EdgeOutputs holds outputs from the public-facing edge stage
// (ALB + Route53 + ACM on AWS; Cloud Load Balancing + Cloud DNS + managed
// certs on GCP).
type EdgeOutputs struct {
	// LoadBalancerDns is the public hostname of the L7 load balancer
	// (ALB DNS name on AWS, GCLB IP-address-derived hostname on GCP).
	LoadBalancerDns pulumi.StringOutput

	// LoadBalancerArn is the AWS ALB ARN. Empty on GCP.
	LoadBalancerArn pulumi.StringOutput

	// CertificateRef is the TLS certificate reference (ACM ARN on AWS,
	// google_compute_managed_ssl_certificate self-link on GCP).
	CertificateRef pulumi.StringOutput

	// HostedZoneId is the DNS hosted zone identifier
	// (Route53 hosted zone ID on AWS, Cloud DNS managed zone name on GCP).
	HostedZoneId pulumi.StringOutput
}

// CICDOutputs holds outputs from the container registry / build stage.
// Not part of the original spec listing but needed by Deploy() to wire
// repository URLs into compute. AWS: ECR repositories. GCP: Artifact Registry.
type CICDOutputs struct {
	// RepositoryURLs maps service name (e.g., "assignment", "management") to
	// the fully-qualified image repository URL. Pull-and-push compatible with
	// `docker push <url>:<tag>`.
	RepositoryURLs map[string]pulumi.StringOutput
}
