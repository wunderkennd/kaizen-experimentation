package streaming

import (
	"encoding/json"
	"fmt"

	"github.com/pulumi/pulumi-aws/sdk/v6/go/aws/cloudwatch"
	"github.com/pulumi/pulumi-aws/sdk/v6/go/aws/ecs"
	"github.com/pulumi/pulumi-aws/sdk/v6/go/aws/iam"
	"github.com/pulumi/pulumi-aws/sdk/v6/go/aws/servicediscovery"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"
)

// SchemaRegistryArgs configures the Confluent Schema Registry ECS Fargate service.
type SchemaRegistryArgs struct {
	// Environment name: "dev", "staging", or "prod".
	Environment string
	// Region is the AWS region for CloudWatch log configuration.
	Region string
	// ClusterArn is the ECS cluster to deploy into.
	ClusterArn pulumi.StringOutput
	// PrivateSubnetIds for Fargate task placement.
	PrivateSubnetIds pulumi.StringArrayOutput
	// SecurityGroupId is the ECS security group.
	SecurityGroupId pulumi.IDOutput
	// NamespaceId is the Cloud Map private DNS namespace ID (kaizen.local).
	NamespaceId pulumi.IDOutput
	// BootstrapBrokers is the MSK SASL/SCRAM bootstrap broker connection string.
	BootstrapBrokers pulumi.StringOutput
	// KafkaSecretArn is the Secrets Manager ARN containing Kafka SASL credentials.
	KafkaSecretArn pulumi.StringOutput
	// Tags applied to all resources.
	Tags pulumi.StringMap
}

// SchemaRegistryOutputs holds the outputs from the Schema Registry ECS service.
type SchemaRegistryOutputs struct {
	// ServiceArn is the ECS service ARN.
	ServiceArn pulumi.StringOutput
	// ServiceName is the ECS service name (used by CloudWatch alarms).
	ServiceName pulumi.StringOutput
	// SchemaRegistryUrl is the internal URL for schema-registry.kaizen.local:8081.
	SchemaRegistryUrl pulumi.StringOutput
}

// ecsAssumeRolePolicy allows the ECS tasks service to assume IAM roles.
const ecsAssumeRolePolicy = `{
  "Version": "2012-10-17",
  "Statement": [{
    "Effect": "Allow",
    "Principal": {"Service": "ecs-tasks.amazonaws.com"},
    "Action": "sts:AssumeRole"
  }]
}`

// NewSchemaRegistry deploys Confluent Schema Registry on ECS Fargate,
// wired to MSK via SASL/SCRAM and registered in Cloud Map as
// schema-registry.kaizen.local:8081.
//
// Task: I.1.4 — 0.25 vCPU / 512 MB, HTTP health check on /subjects,
// Protobuf compatibility mode BACKWARD.
func NewSchemaRegistry(ctx *pulumi.Context, args *SchemaRegistryArgs) (*SchemaRegistryOutputs, error) {
	prefix := fmt.Sprintf("kaizen-%s", args.Environment)

	// --- IAM Execution Role (ECR pull + CloudWatch Logs + Secrets Manager) ---

	execRole, err := iam.NewRole(ctx, "sr-exec-role", &iam.RoleArgs{
		Name:             pulumi.Sprintf("%s-sr-exec-role", prefix),
		AssumeRolePolicy: pulumi.String(ecsAssumeRolePolicy),
		Tags:             args.Tags,
	})
	if err != nil {
		return nil, fmt.Errorf("creating SR execution role: %w", err)
	}

	_, err = iam.NewRolePolicyAttachment(ctx, "sr-exec-ecs-policy", &iam.RolePolicyAttachmentArgs{
		Role:      execRole.Name,
		PolicyArn: pulumi.String("arn:aws:iam::aws:policy/service-role/AmazonECSTaskExecutionRolePolicy"),
	})
	if err != nil {
		return nil, fmt.Errorf("attaching SR execution policy: %w", err)
	}

	// Inline policy: read Kafka SASL credentials from Secrets Manager.
	secretsPolicy := args.KafkaSecretArn.ApplyT(func(arn string) (string, error) {
		policy := map[string]interface{}{
			"Version": "2012-10-17",
			"Statement": []map[string]interface{}{
				{
					"Effect":   "Allow",
					"Action":   []string{"secretsmanager:GetSecretValue"},
					"Resource": arn,
				},
			},
		}
		b, err := json.Marshal(policy)
		return string(b), err
	}).(pulumi.StringOutput)

	_, err = iam.NewRolePolicy(ctx, "sr-exec-secrets-policy", &iam.RolePolicyArgs{
		Role:   execRole.Name,
		Policy: secretsPolicy,
	})
	if err != nil {
		return nil, fmt.Errorf("creating SR secrets policy: %w", err)
	}

	// --- IAM Task Role (running container identity) ---

	taskRole, err := iam.NewRole(ctx, "sr-task-role", &iam.RoleArgs{
		Name:             pulumi.Sprintf("%s-sr-task-role", prefix),
		AssumeRolePolicy: pulumi.String(ecsAssumeRolePolicy),
		Tags:             args.Tags,
	})
	if err != nil {
		return nil, fmt.Errorf("creating SR task role: %w", err)
	}

	// --- CloudWatch Log Group ---

	logGroup, err := cloudwatch.NewLogGroup(ctx, "sr-logs", &cloudwatch.LogGroupArgs{
		Name:            pulumi.Sprintf("/ecs/%s/schema-registry", prefix),
		RetentionInDays: pulumi.Int(logRetentionDays(args.Environment)),
		Tags:            args.Tags,
	})
	if err != nil {
		return nil, fmt.Errorf("creating SR log group: %w", err)
	}

	// --- ECS Task Definition ---
	// 0.25 vCPU / 512 MB Fargate, Confluent Schema Registry 7.5.3,
	// Protobuf compatibility = BACKWARD, health check on :8081/subjects.

	containerDef := pulumi.All(args.BootstrapBrokers, logGroup.Name, args.KafkaSecretArn).ApplyT(
		func(vals []interface{}) (string, error) {
			brokers := vals[0].(string)
			logGroupName := vals[1].(string)
			kafkaSecretArn := vals[2].(string)

			containers := []map[string]interface{}{
				{
					"name":      "schema-registry",
					"image":     "confluentinc/cp-schema-registry:7.5.3",
					"cpu":       256,
					"memory":    512,
					"essential": true,
					"portMappings": []map[string]interface{}{
						{
							"containerPort": 8081,
							"protocol":      "tcp",
						},
					},
					"environment": []map[string]string{
						{"name": "SCHEMA_REGISTRY_HOST_NAME", "value": "0.0.0.0"},
						{"name": "SCHEMA_REGISTRY_LISTENERS", "value": "http://0.0.0.0:8081"},
						{"name": "SCHEMA_REGISTRY_KAFKASTORE_BOOTSTRAP_SERVERS", "value": brokers},
						{"name": "SCHEMA_REGISTRY_KAFKASTORE_SECURITY_PROTOCOL", "value": "SASL_SSL"},
						{"name": "SCHEMA_REGISTRY_KAFKASTORE_SASL_MECHANISM", "value": "SCRAM-SHA-512"},
						{"name": "SCHEMA_REGISTRY_SCHEMA_COMPATIBILITY_LEVEL", "value": "BACKWARD"},
						{"name": "SCHEMA_REGISTRY_KAFKASTORE_TOPIC", "value": "_schemas"},
						{"name": "SCHEMA_REGISTRY_KAFKASTORE_TOPIC_REPLICATION_FACTOR", "value": "3"},
					},
					"secrets": []map[string]string{
						{
							"name":      "SCHEMA_REGISTRY_KAFKASTORE_SASL_JAAS_CONFIG",
							"valueFrom": kafkaSecretArn + ":sasl_jaas_config::",
						},
					},
					"healthCheck": map[string]interface{}{
						"command":     []string{"CMD-SHELL", "curl -f http://localhost:8081/subjects || exit 1"},
						"interval":    30,
						"timeout":     5,
						"retries":     3,
						"startPeriod": 60,
					},
					"logConfiguration": map[string]interface{}{
						"logDriver": "awslogs",
						"options": map[string]string{
							"awslogs-group":         logGroupName,
							"awslogs-region":        args.Region,
							"awslogs-stream-prefix": "schema-registry",
						},
					},
				},
			}
			b, err := json.Marshal(containers)
			return string(b), err
		},
	).(pulumi.StringOutput)

	taskDef, err := ecs.NewTaskDefinition(ctx, "sr-task-def", &ecs.TaskDefinitionArgs{
		Family:                  pulumi.Sprintf("%s-schema-registry", prefix),
		NetworkMode:             pulumi.String("awsvpc"),
		RequiresCompatibilities: pulumi.StringArray{pulumi.String("FARGATE")},
		Cpu:                     pulumi.String("256"),
		Memory:                  pulumi.String("512"),
		ExecutionRoleArn:        execRole.Arn,
		TaskRoleArn:             taskRole.Arn,
		ContainerDefinitions:    containerDef,
		Tags:                    args.Tags,
	})
	if err != nil {
		return nil, fmt.Errorf("creating SR task definition: %w", err)
	}

	// --- Cloud Map Service Discovery ---
	// Registers as schema-registry.kaizen.local, A record with 10s TTL.

	cmService, err := servicediscovery.NewService(ctx, "sr-discovery", &servicediscovery.ServiceArgs{
		Name:        pulumi.String("schema-registry"),
		NamespaceId: args.NamespaceId.ToStringOutput(),
		DnsConfig: &servicediscovery.ServiceDnsConfigArgs{
			DnsRecords: servicediscovery.ServiceDnsConfigDnsRecordArray{
				&servicediscovery.ServiceDnsConfigDnsRecordArgs{
					Type: pulumi.String("A"),
					Ttl:  pulumi.Int(10),
				},
			},
			RoutingPolicy: pulumi.String("MULTIVALUE"),
		},
		HealthCheckCustomConfig: &servicediscovery.ServiceHealthCheckCustomConfigArgs{
			FailureThreshold: pulumi.Int(1),
		},
		Tags: args.Tags,
	})
	if err != nil {
		return nil, fmt.Errorf("creating SR Cloud Map service: %w", err)
	}

	// --- ECS Service ---

	svc, err := ecs.NewService(ctx, "sr-service", &ecs.ServiceArgs{
		Name:           pulumi.Sprintf("%s-schema-registry", prefix),
		Cluster:        args.ClusterArn,
		TaskDefinition: taskDef.Arn,
		DesiredCount:   pulumi.Int(1),
		LaunchType:     pulumi.String("FARGATE"),

		// Deployment circuit breaker: rolls back automatically if the
		// container health check (HTTP GET :8081/subjects) keeps failing.
		DeploymentCircuitBreaker: &ecs.ServiceDeploymentCircuitBreakerArgs{
			Enable:   pulumi.Bool(true),
			Rollback: pulumi.Bool(true),
		},

		NetworkConfiguration: &ecs.ServiceNetworkConfigurationArgs{
			Subnets:        args.PrivateSubnetIds,
			SecurityGroups: pulumi.StringArray{args.SecurityGroupId.ToStringOutput()},
			AssignPublicIp: pulumi.Bool(false),
		},

		ServiceRegistries: &ecs.ServiceServiceRegistriesArgs{
			RegistryArn:   cmService.Arn,
			ContainerName: pulumi.String("schema-registry"),
			ContainerPort: pulumi.Int(8081),
		},

		Tags: args.Tags,
	})
	if err != nil {
		return nil, fmt.Errorf("creating SR ECS service: %w", err)
	}

	// Suppress unused warning for task role (will gain policies in future sprints).
	_ = taskRole

	return &SchemaRegistryOutputs{
		ServiceArn:        svc.ID().ToStringOutput(),
		ServiceName:       pulumi.Sprintf("%s-schema-registry", prefix),
		SchemaRegistryUrl: pulumi.Sprintf("http://schema-registry.kaizen.local:8081"),
	}, nil
}
