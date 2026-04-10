// Package compute — migration.go provisions an ECS Fargate task definition
// and Pulumi Command trigger for running database migrations as a pre-deploy
// step. The migration container uses the M5 management image (which bundles
// sql/migrations/*.sql) with a command override that runs golang-migrate
// against RDS PostgreSQL.
//
// Sprint I.2.2 scope: task definition, execution role, log group, and a
// local.Command that calls `aws ecs run-task`, waits for completion, and
// exits non-zero on failure — blocking the M5 service deployment.
package compute

import (
	"encoding/json"
	"fmt"
	"strings"

	"github.com/pulumi/pulumi-aws/sdk/v6/go/aws/cloudwatch"
	"github.com/pulumi/pulumi-aws/sdk/v6/go/aws/ecs"
	"github.com/pulumi/pulumi-aws/sdk/v6/go/aws/iam"
	"github.com/pulumi/pulumi-command/sdk/go/command/local"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"
)

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

// MigrationArgs holds the inputs for the database migration ECS task.
type MigrationArgs struct {
	// Environment name: "dev", "staging", or "prod".
	Environment string
	// ClusterArn from the ECS cluster.
	ClusterArn pulumi.StringOutput
	// PrivateSubnetIds for Fargate task networking.
	PrivateSubnetIds pulumi.StringArrayOutput
	// SecurityGroupId for ECS tasks.
	SecurityGroupId pulumi.IDOutput
	// ECRRepositoryURL for the M5 management image (contains sql/migrations/).
	ECRRepositoryURL pulumi.StringOutput
	// DatabaseSecretArn is the Secrets Manager ARN holding PG credentials.
	DatabaseSecretArn pulumi.StringOutput
	// Region is the AWS region (e.g., "us-east-1").
	Region string
}

// MigrationOutputs holds the resources created by NewMigration.
type MigrationOutputs struct {
	// TaskDefinitionArn is the ARN of the migration ECS task definition.
	TaskDefinitionArn pulumi.StringOutput
	// RunCommand is the Pulumi Command resource that triggers the migration.
	// Pass this to ServicesArgs.PreDeployDeps so M5 waits for migration.
	RunCommand pulumi.Resource
}

// ---------------------------------------------------------------------------
// Constructor
// ---------------------------------------------------------------------------

// NewMigration creates an ECS Fargate task definition for database migrations
// and a Pulumi Command that triggers the task on each deployment. The task
// reads sql/migrations/*.sql from the M5 management container image and runs
// golang-migrate against RDS PostgreSQL.
//
// The returned RunCommand resource should be added to ServicesArgs.PreDeployDeps
// so that the M5 management service does not start until migrations succeed.
func NewMigration(ctx *pulumi.Context, args *MigrationArgs) (*MigrationOutputs, error) {
	prefix := fmt.Sprintf("kaizen-%s", args.Environment)

	// --- Execution role: ECR pull + CloudWatch Logs + Secrets Manager ---
	execRole, err := newMigrationExecRole(ctx, prefix, args.DatabaseSecretArn)
	if err != nil {
		return nil, fmt.Errorf("creating migration execution role: %w", err)
	}

	// --- Dedicated log group for migration output ---
	logGroup, err := cloudwatch.NewLogGroup(ctx, "migration-logs", &cloudwatch.LogGroupArgs{
		Name:            pulumi.Sprintf("/ecs/%s/migration", prefix),
		RetentionInDays: pulumi.Int(logRetentionDays(args.Environment)),
		Tags: pulumi.StringMap{
			"Project":     pulumi.String("kaizen"),
			"Environment": pulumi.String(args.Environment),
			"Service":     pulumi.String("db-migration"),
			"ManagedBy":   pulumi.String("pulumi"),
		},
	})
	if err != nil {
		return nil, fmt.Errorf("creating migration log group: %w", err)
	}

	// --- Container definition ---
	containerDefsJSON := buildMigrationContainerDefs(args, logGroup, prefix)

	// --- Task definition ---
	taskDef, err := ecs.NewTaskDefinition(ctx, "td-db-migration", &ecs.TaskDefinitionArgs{
		Family:                  pulumi.Sprintf("%s-db-migration", prefix),
		Cpu:                     pulumi.String("256"),
		Memory:                  pulumi.String("512"),
		NetworkMode:             pulumi.String("awsvpc"),
		RequiresCompatibilities: pulumi.StringArray{pulumi.String("FARGATE")},
		ExecutionRoleArn:        execRole.Arn,
		ContainerDefinitions:    containerDefsJSON,
		Tags: pulumi.StringMap{
			"Project":     pulumi.String("kaizen"),
			"Environment": pulumi.String(args.Environment),
			"Service":     pulumi.String("db-migration"),
			"ManagedBy":   pulumi.String("pulumi"),
		},
	})
	if err != nil {
		return nil, fmt.Errorf("creating migration task definition: %w", err)
	}

	// --- Trigger migration on deployment ---
	runCmd, err := newMigrationCommand(ctx, args, taskDef)
	if err != nil {
		return nil, fmt.Errorf("creating migration command: %w", err)
	}

	ctx.Export("migrationTaskDefinitionArn", taskDef.Arn)

	return &MigrationOutputs{
		TaskDefinitionArn: taskDef.Arn,
		RunCommand:        runCmd,
	}, nil
}

// ---------------------------------------------------------------------------
// IAM
// ---------------------------------------------------------------------------

// newMigrationExecRole creates a focused IAM execution role for the migration
// task. Grants ECR image pull, CloudWatch Logs write, and Secrets Manager
// read access for database credentials only.
func newMigrationExecRole(
	ctx *pulumi.Context,
	prefix string,
	dbSecretArn pulumi.StringOutput,
) (*iam.Role, error) {
	assumePolicy := `{
  "Version": "2012-10-17",
  "Statement": [{
    "Effect": "Allow",
    "Principal": {"Service": "ecs-tasks.amazonaws.com"},
    "Action": "sts:AssumeRole"
  }]
}`

	role, err := iam.NewRole(ctx, "migration-exec-role", &iam.RoleArgs{
		Name:             pulumi.Sprintf("%s-migration-exec", prefix),
		AssumeRolePolicy: pulumi.String(assumePolicy),
		Tags: pulumi.StringMap{
			"Project":     pulumi.String("kaizen"),
			"Service":     pulumi.String("db-migration"),
			"ManagedBy":   pulumi.String("pulumi"),
		},
	})
	if err != nil {
		return nil, fmt.Errorf("creating role: %w", err)
	}

	// Managed policy: ECR pull + CloudWatch Logs.
	_, err = iam.NewRolePolicyAttachment(ctx, "migration-exec-managed", &iam.RolePolicyAttachmentArgs{
		Role:      role.Name,
		PolicyArn: pulumi.String("arn:aws:iam::aws:policy/service-role/AmazonECSTaskExecutionRolePolicy"),
	})
	if err != nil {
		return nil, fmt.Errorf("attaching managed policy: %w", err)
	}

	// Inline policy: read database secret (including individual JSON keys).
	secretsPolicy := dbSecretArn.ApplyT(func(arn string) (string, error) {
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

	_, err = iam.NewRolePolicy(ctx, "migration-exec-secrets", &iam.RolePolicyArgs{
		Name:   pulumi.Sprintf("%s-migration-secrets", prefix),
		Role:   role.ID(),
		Policy: secretsPolicy,
	})
	if err != nil {
		return nil, fmt.Errorf("attaching secrets policy: %w", err)
	}

	return role, nil
}

// ---------------------------------------------------------------------------
// Container definition
// ---------------------------------------------------------------------------

// buildMigrationContainerDefs constructs the JSON container definition for the
// migration task. The container uses the M5 management image with entrypoint
// and command overrides to run golang-migrate.
//
// Database credentials are injected via ECS Secrets Manager integration using
// JSON key extraction (e.g., "arn:...:host::") so the container does not need
// jq or other JSON parsing tools.
func buildMigrationContainerDefs(
	args *MigrationArgs,
	logGroup *cloudwatch.LogGroup,
	prefix string,
) pulumi.StringOutput {
	return pulumi.All(
		args.ECRRepositoryURL,
		logGroup.Name,
		args.DatabaseSecretArn,
	).ApplyT(func(vals []interface{}) (string, error) {
		imageURL := vals[0].(string)
		logGroupName := vals[1].(string)
		dbSecretArn := vals[2].(string)

		// The migrate command constructs a PostgreSQL URL from individual
		// secret fields injected by ECS. DB_HOST is the RDS endpoint
		// (hostname:port format), so it includes the port already.
		migrateCmd := `set -e; echo "Running database migrations..."; migrate -path /app/sql/migrations -database "postgres://${DB_USER}:${DB_PASS}@${DB_HOST}/${DB_NAME}?sslmode=require" up; echo "Migrations complete."`

		def := containerDef{
			Name:       "db-migration",
			Image:      imageURL + ":latest",
			Essential:  true,
			EntryPoint: []string{"sh", "-c"},
			Command:    []string{migrateCmd},
			PortMappings: []portMap{},
			LogConfiguration: logCfg{
				LogDriver: "awslogs",
				Options: map[string]string{
					"awslogs-group":         logGroupName,
					"awslogs-region":        args.Region,
					"awslogs-stream-prefix": "db-migration",
				},
			},
			Environment: []envKV{
				{Name: "ENVIRONMENT", Value: args.Environment},
			},
			Secrets: []secretRef{
				{Name: "DB_HOST", ValueFrom: dbSecretArn + ":host::"},
				{Name: "DB_USER", ValueFrom: dbSecretArn + ":username::"},
				{Name: "DB_PASS", ValueFrom: dbSecretArn + ":password::"},
				{Name: "DB_NAME", ValueFrom: dbSecretArn + ":dbname::"},
			},
		}

		b, err := json.Marshal([]containerDef{def})
		if err != nil {
			return "", fmt.Errorf("marshaling migration container def: %w", err)
		}
		return string(b), nil
	}).(pulumi.StringOutput)
}

// ---------------------------------------------------------------------------
// Deployment trigger
// ---------------------------------------------------------------------------

// newMigrationCommand creates a Pulumi Command resource that triggers the
// migration ECS task on each deployment. It runs `aws ecs run-task`, waits
// for the task to stop, and checks the container exit code.
//
// The command re-runs whenever the task definition ARN changes (which happens
// when the container image or configuration is updated).
//
// Prerequisites: the machine running `pulumi up` must have the AWS CLI
// configured with credentials that can run ECS tasks.
func newMigrationCommand(
	ctx *pulumi.Context,
	args *MigrationArgs,
	taskDef *ecs.TaskDefinition,
) (*local.Command, error) {
	createScript := pulumi.All(
		args.ClusterArn,
		taskDef.Arn,
		args.PrivateSubnetIds,
		args.SecurityGroupId.ToStringOutput(),
	).ApplyT(func(vals []interface{}) (string, error) {
		cluster := vals[0].(string)
		taskDefArn := vals[1].(string)
		sg := vals[3].(string)

		// Resolve subnets — StringArrayOutput resolves to []string.
		var subnetStrs []string
		switch v := vals[2].(type) {
		case []string:
			subnetStrs = v
		case []interface{}:
			subnetStrs = make([]string, len(v))
			for i, s := range v {
				subnetStrs[i] = s.(string)
			}
		}
		subnetsCSV := strings.Join(subnetStrs, ",")

		return fmt.Sprintf(`set -euo pipefail

echo "==> Starting database migration task..."
TASK_ARN=$(aws ecs run-task \
  --cluster %q \
  --task-definition %q \
  --launch-type FARGATE \
  --network-configuration 'awsvpcConfiguration={subnets=[%s],securityGroups=[%s],assignPublicIp=DISABLED}' \
  --query 'tasks[0].taskArn' \
  --output text)

if [ -z "$TASK_ARN" ] || [ "$TASK_ARN" = "None" ]; then
  echo "ERROR: Failed to start migration task"
  exit 1
fi

echo "==> Migration task started: $TASK_ARN"
echo "==> Waiting for task to complete..."

aws ecs wait tasks-stopped \
  --cluster %q \
  --tasks "$TASK_ARN"

EXIT_CODE=$(aws ecs describe-tasks \
  --cluster %q \
  --tasks "$TASK_ARN" \
  --query 'tasks[0].containers[0].exitCode' \
  --output text)

if [ "$EXIT_CODE" = "0" ]; then
  echo "==> Database migration completed successfully."
else
  echo "ERROR: Database migration failed (exit code: $EXIT_CODE)"
  REASON=$(aws ecs describe-tasks \
    --cluster %q \
    --tasks "$TASK_ARN" \
    --query 'tasks[0].stoppedReason' \
    --output text)
  echo "==> Stop reason: $REASON"
  exit 1
fi
`, cluster, taskDefArn, subnetsCSV, sg, cluster, cluster, cluster), nil
	}).(pulumi.StringOutput)

	return local.NewCommand(ctx, "run-db-migration", &local.CommandArgs{
		Create: createScript,
		Triggers: pulumi.Array{
			taskDef.Arn,
		},
	})
}
