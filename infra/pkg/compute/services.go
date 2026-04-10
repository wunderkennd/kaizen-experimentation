// Package compute — services.go provisions ECS Fargate task definitions and
// services for all 8 Fargate-based Kaizen platform modules.
//
// M4b (Policy) runs on EC2 via the capacity provider in cluster.go.
// This file handles M1, M2, M2-Orch, M3, M4a, M5, M6, M7.
//
// Sprint I.1.5 scope: task defs, services, Cloud Map registration,
// awslogs driver, env vars from Secrets Manager + Cloud Map DNS names,
// and health checks per service type.
package compute

import (
	"encoding/json"
	"fmt"
	"sort"
	"strings"

	"github.com/pulumi/pulumi-aws/sdk/v6/go/aws"
	"github.com/pulumi/pulumi-aws/sdk/v6/go/aws/cloudwatch"
	"github.com/pulumi/pulumi-aws/sdk/v6/go/aws/ecs"
	"github.com/pulumi/pulumi-aws/sdk/v6/go/aws/iam"
	"github.com/pulumi/pulumi-aws/sdk/v6/go/aws/servicediscovery"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"
)

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

// ServicesArgs holds the cross-module inputs required to provision ECS services.
type ServicesArgs struct {
	// Environment name: "dev", "staging", or "prod".
	Environment string
	// ClusterArn from the ECS cluster (compute.NewCluster).
	ClusterArn pulumi.StringOutput
	// PrivateSubnetIds for Fargate task networking.
	PrivateSubnetIds pulumi.StringArrayOutput
	// SecurityGroupId for ECS Fargate tasks (network.SecurityGroups["ecs"]).
	SecurityGroupId pulumi.IDOutput
	// NamespaceId from the Cloud Map private DNS namespace.
	NamespaceId pulumi.IDOutput
	// ECRRepositoryURLs maps service key → ECR repository URL.
	ECRRepositoryURLs map[string]pulumi.StringOutput
	// Secret ARNs from the secrets module.
	DatabaseSecretArn pulumi.StringOutput
	KafkaSecretArn    pulumi.StringOutput
	RedisSecretArn    pulumi.StringOutput
	AuthSecretArn     pulumi.StringOutput
	// DesiredCount is the initial task count per service.
	DesiredCount int
}

// ServicesOutputs holds the outputs from ECS service provisioning.
type ServicesOutputs struct {
	// ServiceArns maps service key → ECS service ARN.
	// Keys: "m1", "m2", "m2-orch", "m3", "m4a", "m5", "m6", "m7"
	ServiceArns map[string]pulumi.StringOutput
	// TaskRoleArn is the IAM role assumed by running containers.
	TaskRoleArn pulumi.StringOutput
	// ExecRoleArn is the IAM role used by ECS to pull images and push logs.
	ExecRoleArn pulumi.StringOutput
	// M5ServiceResource exposes the M5 ECS service for dependency wiring.
	// Downstream modules (e.g., M4b) can use pulumi.DependsOn with this.
	M5ServiceResource pulumi.Resource
	// Tier1Resources lists all Tier 1 ECS service resources, used for
	// external dependency wiring (e.g., M4b Cloud Map depends on Tier 1).
	Tier1Resources []pulumi.Resource
}

// ---------------------------------------------------------------------------
// Service specification table
// ---------------------------------------------------------------------------

// Startup tiers control service deployment ordering. Higher tiers wait for
// all services in lower tiers to reach steady state before deploying.
//
//	Tier 0: M5 — owns PostgreSQL schema via migration, must start first.
//	Tier 1: M1, M2, M2-Orch — core assignment/pipeline services, depend on M5.
//	        M4b (EC2) is logically Tier 1 but created separately in main.go.
//	Tier 2: M3, M4a, M6, M7 — depend on Tier 1 services + M4b.
const (
	TierFoundation = 0
	TierCore       = 1
	TierDependent  = 2
)

// serviceSpec defines one Fargate service declaratively.
type serviceSpec struct {
	key       string // output map key: "m1", "m2", etc.
	name      string // resource/Cloud Map name: "m1-assignment"
	ecrKey    string // key into ECRRepositoryURLs
	cpu       string // Fargate CPU units
	memoryMB  string // Fargate memory in MB
	ports     []int  // container ports
	lang      string // "rust", "go", "ts" — determines health check
	healthCmd []string
	tier      int         // startup tier (TierFoundation, TierCore, TierDependent)
	deps      []healthDep // upstream services to health-gate on at runtime
}

// healthDep describes an upstream service endpoint that a health-gate init
// container must verify before the main container starts.
type healthDep struct {
	name  string // human-readable label (for log messages)
	host  string // Cloud Map DNS name
	port  int
	proto string // "http" (wget) or "tcp" (nc)
	path  string // HTTP health path (e.g., "/healthz"); ignored for "tcp"
}

// m5Dep is the health dependency on M5 Management (HTTP healthz).
var m5Dep = healthDep{name: "M5", host: "m5-management.kaizen.local", port: 50055, proto: "http", path: "/healthz"}

// tier1Deps are the health dependencies on Tier 1 services (M1, M2, M4b).
// Used by Tier 2 services. M4b is included because it is logically Tier 1.
var tier1Deps = []healthDep{
	{name: "M1", host: "m1-assignment.kaizen.local", port: 50051, proto: "tcp"},
	{name: "M2", host: "m2-pipeline.kaizen.local", port: 50052, proto: "tcp"},
	{name: "M4b", host: "m4b-policy.kaizen.local", port: 50054, proto: "tcp"},
}

func serviceSpecs() []serviceSpec {
	return []serviceSpec{
		// ── Tier 0: Foundation ── M5 starts first (owns PG schema via migration)
		{
			key: "m5", name: "m5-management", ecrKey: "management",
			cpu: "512", memoryMB: "1024", ports: []int{50055, 50060},
			lang:      "go",
			healthCmd: []string{"CMD-SHELL", "wget --spider -q http://localhost:50055/healthz || exit 1"},
			tier:      TierFoundation,
			deps:      nil, // No upstream dependencies.
		},
		// ── Tier 1: Core ── M1, M2, M2-Orch start after M5 is healthy
		{
			key: "m1", name: "m1-assignment", ecrKey: "assignment",
			cpu: "512", memoryMB: "1024", ports: []int{50051},
			lang:      "rust",
			healthCmd: []string{"CMD", "/bin/grpc_health_probe", "-addr=:50051"},
			tier:      TierCore,
			deps:      []healthDep{m5Dep},
		},
		{
			key: "m2", name: "m2-pipeline", ecrKey: "pipeline",
			cpu: "512", memoryMB: "1024", ports: []int{50052},
			lang:      "rust",
			healthCmd: []string{"CMD", "/bin/grpc_health_probe", "-addr=:50052"},
			tier:      TierCore,
			deps:      []healthDep{m5Dep},
		},
		{
			key: "m2-orch", name: "m2-orchestration", ecrKey: "orchestration",
			cpu: "256", memoryMB: "512", ports: []int{50058},
			lang:      "go",
			healthCmd: []string{"CMD-SHELL", "wget --spider -q http://localhost:50058/healthz || exit 1"},
			tier:      TierCore,
			deps:      []healthDep{m5Dep},
		},
		// ── Tier 2: Dependent ── M3, M4a, M6, M7 start after Tier 1 + M4b
		{
			key: "m3", name: "m3-metrics", ecrKey: "metrics",
			cpu: "1024", memoryMB: "2048", ports: []int{50056, 50059},
			lang:      "go",
			healthCmd: []string{"CMD-SHELL", "wget --spider -q http://localhost:50056/healthz || exit 1"},
			tier:      TierDependent,
			deps:      tier1Deps,
		},
		{
			key: "m4a", name: "m4a-analysis", ecrKey: "analysis",
			cpu: "1024", memoryMB: "2048", ports: []int{50053},
			lang:      "rust",
			healthCmd: []string{"CMD", "/bin/grpc_health_probe", "-addr=:50053"},
			tier:      TierDependent,
			deps:      tier1Deps,
		},
		{
			key: "m6", name: "m6-ui", ecrKey: "ui",
			cpu: "512", memoryMB: "1024", ports: []int{3000},
			lang:      "ts",
			healthCmd: []string{"CMD-SHELL", "wget --spider -q http://localhost:3000/ || exit 1"},
			tier:      TierDependent,
			deps:      tier1Deps,
		},
		{
			key: "m7", name: "m7-flags", ecrKey: "flags",
			cpu: "256", memoryMB: "512", ports: []int{50057},
			lang:      "rust",
			healthCmd: []string{"CMD", "/bin/grpc_health_probe", "-addr=:50057"},
			tier:      TierDependent,
			deps:      tier1Deps,
		},
	}
}

// serviceEndpoints returns Cloud Map DNS names for all services (including
// EC2-based M4b) so every container can discover every other service.
func serviceEndpoints() map[string]string {
	return map[string]string{
		"M1_ASSIGNMENT_ENDPOINT":    "m1-assignment.kaizen.local:50051",
		"M2_PIPELINE_ENDPOINT":      "m2-pipeline.kaizen.local:50052",
		"M2_ORCHESTRATION_ENDPOINT": "m2-orchestration.kaizen.local:50058",
		"M3_METRICS_ENDPOINT":       "m3-metrics.kaizen.local:50056",
		"M4A_ANALYSIS_ENDPOINT":     "m4a-analysis.kaizen.local:50053",
		"M4B_POLICY_ENDPOINT":       "m4b-policy.kaizen.local:50054",
		"M5_MANAGEMENT_ENDPOINT":    "m5-management.kaizen.local:50055",
		"M6_UI_ENDPOINT":            "m6-ui.kaizen.local:3000",
		"M7_FLAGS_ENDPOINT":         "m7-flags.kaizen.local:50057",
	}
}

// ---------------------------------------------------------------------------
// Container definition types (for JSON serialization)
// ---------------------------------------------------------------------------

type containerDef struct {
	Name             string                `json:"name"`
	Image            string                `json:"image"`
	Essential        bool                  `json:"essential"`
	PortMappings     []portMap             `json:"portMappings,omitempty"`
	LogConfiguration logCfg                `json:"logConfiguration"`
	Environment      []envKV               `json:"environment"`
	Secrets          []secretRef           `json:"secrets,omitempty"`
	HealthCheck      *healthCheck          `json:"healthCheck,omitempty"`
	DependsOn        []containerDependency `json:"dependsOn,omitempty"`
	Command          []string              `json:"command,omitempty"`
}

// containerDependency defines an intra-task dependency between containers.
// Used to chain the health-gate init container to the main application container.
type containerDependency struct {
	ContainerName string `json:"containerName"`
	Condition     string `json:"condition"` // "SUCCESS", "HEALTHY", "START", "COMPLETE"
}

type portMap struct {
	ContainerPort int    `json:"containerPort"`
	Protocol      string `json:"protocol"`
}

type logCfg struct {
	LogDriver string            `json:"logDriver"`
	Options   map[string]string `json:"options"`
}

type envKV struct {
	Name  string `json:"name"`
	Value string `json:"value"`
}

type secretRef struct {
	Name      string `json:"name"`
	ValueFrom string `json:"valueFrom"`
}

type healthCheck struct {
	Command     []string `json:"command"`
	Interval    int      `json:"interval"`
	Timeout     int      `json:"timeout"`
	Retries     int      `json:"retries"`
	StartPeriod int      `json:"startPeriod"`
}

// ---------------------------------------------------------------------------
// Constructor
// ---------------------------------------------------------------------------

// NewServices creates 8 ECS Fargate task definitions and services for the
// Kaizen platform, deployed in dependency tiers:
//
//	Tier 0 (Foundation): M5 — deployed first, waits for steady state.
//	Tier 1 (Core):       M1, M2, M2-Orch — deployed after Tier 0 is healthy.
//	Tier 2 (Dependent):  M3, M4a, M6, M7 — deployed after Tier 1 is healthy.
//
// Each service gets Cloud Map registration, structured logging via awslogs,
// environment variables for service discovery, secrets injected from Secrets
// Manager, and (for Tier 1+) health-gate init containers that poll upstream
// services before the main container starts.
func NewServices(ctx *pulumi.Context, args *ServicesArgs) (*ServicesOutputs, error) {
	prefix := fmt.Sprintf("kaizen-%s", args.Environment)

	if args.DesiredCount == 0 {
		args.DesiredCount = 1
	}

	region, err := aws.GetRegion(ctx, &aws.GetRegionArgs{})
	if err != nil {
		return nil, fmt.Errorf("getting AWS region: %w", err)
	}

	// --- IAM roles ---

	execRole, err := newExecutionRole(ctx, prefix, args)
	if err != nil {
		return nil, err
	}

	taskRole, err := newTaskRole(ctx, prefix)
	if err != nil {
		return nil, err
	}

	// --- CloudWatch log group ---

	logGroup, err := cloudwatch.NewLogGroup(ctx, fmt.Sprintf("%s-ecs-logs", prefix), &cloudwatch.LogGroupArgs{
		Name:            pulumi.Sprintf("/ecs/%s", prefix),
		RetentionInDays: pulumi.Int(logRetentionDays(args.Environment)),
		Tags: pulumi.StringMap{
			"Project":     pulumi.String("kaizen"),
			"Environment": pulumi.String(args.Environment),
			"ManagedBy":   pulumi.String("pulumi"),
		},
	})
	if err != nil {
		return nil, fmt.Errorf("creating log group: %w", err)
	}

	// --- Create services tier by tier ---

	specs := serviceSpecs()
	tierMap := groupSpecsByTier(specs)
	serviceArns := make(map[string]pulumi.StringOutput, len(specs))
	ecsServices := make(map[string]*ecs.Service, len(specs))

	// Ordered tier keys for deterministic iteration.
	tierKeys := sortedTierKeys(tierMap)

	for _, tier := range tierKeys {
		tierSpecs := tierMap[tier]

		// Collect Pulumi resource dependencies from the previous tier.
		var pulumiDeps []pulumi.Resource
		if tier > 0 {
			if prevSpecs, ok := tierMap[tier-1]; ok {
				for _, prev := range prevSpecs {
					if svc, ok := ecsServices[prev.key]; ok {
						pulumiDeps = append(pulumiDeps, svc)
					}
				}
			}
		}

		// Tier 0 and Tier 1 block Pulumi until steady state so downstream
		// tiers only deploy after upstream tasks are actually healthy.
		waitForSteady := tier < TierDependent

		for _, spec := range tierSpecs {
			svc, err := newFargateService(ctx, prefix, region.Name, spec, args,
				execRole, taskRole, logGroup, pulumiDeps, waitForSteady)
			if err != nil {
				return nil, fmt.Errorf("creating service %s: %w", spec.name, err)
			}
			serviceArns[spec.key] = svc.ID().ToStringOutput()
			ecsServices[spec.key] = svc
		}
	}

	// Collect Tier 1 resources for external wiring (M4b depends on these).
	var tier1Resources []pulumi.Resource
	if coreSpecs, ok := tierMap[TierCore]; ok {
		for _, s := range coreSpecs {
			if svc, ok := ecsServices[s.key]; ok {
				tier1Resources = append(tier1Resources, svc)
			}
		}
	}

	var m5Resource pulumi.Resource
	if svc, ok := ecsServices["m5"]; ok {
		m5Resource = svc
	}

	return &ServicesOutputs{
		ServiceArns:       serviceArns,
		TaskRoleArn:       taskRole.Arn,
		ExecRoleArn:       execRole.Arn,
		M5ServiceResource: m5Resource,
		Tier1Resources:    tier1Resources,
	}, nil
}

// groupSpecsByTier partitions service specs by their startup tier.
func groupSpecsByTier(specs []serviceSpec) map[int][]serviceSpec {
	m := make(map[int][]serviceSpec)
	for _, s := range specs {
		m[s.tier] = append(m[s.tier], s)
	}
	return m
}

// sortedTierKeys returns tier numbers in ascending order.
func sortedTierKeys(m map[int][]serviceSpec) []int {
	keys := make([]int, 0, len(m))
	for k := range m {
		keys = append(keys, k)
	}
	sort.Ints(keys)
	return keys
}

// ---------------------------------------------------------------------------
// Per-service provisioning
// ---------------------------------------------------------------------------

// newFargateService creates the Cloud Map service, task definition, and ECS
// service for a single Fargate-based module.
//
// pulumiDeps are ECS service resources from a lower tier that this service
// depends on (Pulumi deployment ordering). waitForSteady makes Pulumi block
// until the ECS service reaches steady state (tasks healthy).
func newFargateService(
	ctx *pulumi.Context,
	prefix string,
	awsRegion string,
	spec serviceSpec,
	args *ServicesArgs,
	execRole *iam.Role,
	taskRole *iam.Role,
	logGroup *cloudwatch.LogGroup,
	pulumiDeps []pulumi.Resource,
	waitForSteady bool,
) (*ecs.Service, error) {
	resourcePrefix := fmt.Sprintf("%s-%s", prefix, spec.name)

	// --- Cloud Map service ---

	cmSvc, err := servicediscovery.NewService(ctx, fmt.Sprintf("cm-%s", spec.name), &servicediscovery.ServiceArgs{
		Name: pulumi.String(spec.name),
		DnsConfig: &servicediscovery.ServiceDnsConfigArgs{
			NamespaceId: args.NamespaceId.ToStringOutput(),
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
		Tags: pulumi.StringMap{
			"Project":     pulumi.String("kaizen"),
			"Environment": pulumi.String(args.Environment),
			"Service":     pulumi.String(spec.name),
			"ManagedBy":   pulumi.String("pulumi"),
		},
	})
	if err != nil {
		return nil, fmt.Errorf("Cloud Map service %s: %w", spec.name, err)
	}

	// --- Task definition ---

	containerDefsJSON := buildContainerDefsJSON(spec, args, logGroup, awsRegion)

	taskDef, err := ecs.NewTaskDefinition(ctx, fmt.Sprintf("td-%s", spec.name), &ecs.TaskDefinitionArgs{
		Family:                  pulumi.String(resourcePrefix),
		Cpu:                     pulumi.String(spec.cpu),
		Memory:                  pulumi.String(spec.memoryMB),
		NetworkMode:             pulumi.String("awsvpc"),
		RequiresCompatibilities: pulumi.StringArray{pulumi.String("FARGATE")},
		ExecutionRoleArn:        execRole.Arn,
		TaskRoleArn:             taskRole.Arn,
		ContainerDefinitions:    containerDefsJSON,
		Tags: pulumi.StringMap{
			"Project":     pulumi.String("kaizen"),
			"Environment": pulumi.String(args.Environment),
			"Service":     pulumi.String(spec.name),
			"ManagedBy":   pulumi.String("pulumi"),
		},
	})
	if err != nil {
		return nil, fmt.Errorf("task definition %s: %w", spec.name, err)
	}

	// --- ECS service ---

	svcArgs := &ecs.ServiceArgs{
		Name:           pulumi.String(resourcePrefix),
		Cluster:        args.ClusterArn,
		TaskDefinition: taskDef.Arn,
		DesiredCount:   pulumi.Int(args.DesiredCount),
		LaunchType:     pulumi.String("FARGATE"),

		NetworkConfiguration: &ecs.ServiceNetworkConfigurationArgs{
			Subnets:        args.PrivateSubnetIds,
			SecurityGroups: pulumi.StringArray{args.SecurityGroupId.ToStringOutput()},
			AssignPublicIp: pulumi.Bool(false),
		},

		ServiceRegistries: &ecs.ServiceServiceRegistriesArgs{
			RegistryArn: cmSvc.Arn,
		},

		// Roll out new task before stopping old one (min healthy 100%).
		DeploymentMinimumHealthyPercent: pulumi.Int(100),
		DeploymentMaximumPercent:        pulumi.Int(200),

		// Circuit breaker: auto-rollback if new tasks fail to stabilize.
		DeploymentCircuitBreaker: &ecs.ServiceDeploymentCircuitBreakerArgs{
			Enable:   pulumi.Bool(true),
			Rollback: pulumi.Bool(true),
		},

		// Enable ECS Exec for debugging via `aws ecs execute-command`.
		EnableExecuteCommand: pulumi.Bool(true),

		PropagateTags: pulumi.String("SERVICE"),

		Tags: pulumi.StringMap{
			"Project":     pulumi.String("kaizen"),
			"Environment": pulumi.String(args.Environment),
			"Service":     pulumi.String(spec.name),
			"Tier":        pulumi.Sprintf("%d", spec.tier),
			"ManagedBy":   pulumi.String("pulumi"),
		},
	}

	// WaitForSteadyState blocks Pulumi until ECS tasks are running and healthy.
	// Applied to Tier 0 and Tier 1 so downstream tiers only deploy after
	// upstream services are truly ready (not just API-created).
	if waitForSteady {
		svcArgs.WaitForSteadyState = pulumi.Bool(true)
	}

	// Health-gate containers add a grace period for the init container to
	// poll upstream dependencies before the main container's health check runs.
	if len(spec.deps) > 0 {
		svcArgs.HealthCheckGracePeriodSeconds = pulumi.Int(120)
	}

	// Build Pulumi resource options: DependsOn from previous tier.
	var opts []pulumi.ResourceOption
	if len(pulumiDeps) > 0 {
		opts = append(opts, pulumi.DependsOn(pulumiDeps))
	}

	ecsSvc, err := ecs.NewService(ctx, fmt.Sprintf("svc-%s", spec.name), svcArgs, opts...)
	if err != nil {
		return nil, fmt.Errorf("ECS service %s: %w", spec.name, err)
	}

	return ecsSvc, nil
}

// ---------------------------------------------------------------------------
// Container definition builder
// ---------------------------------------------------------------------------

// buildContainerDefsJSON constructs the JSON container definitions string,
// composing Pulumi outputs from ECR, Secrets Manager, and CloudWatch.
//
// For services with upstream dependencies (tier > 0), an additional
// "health-gate" init container is prepended. The main container's dependsOn
// ensures it waits for the gate to exit successfully before starting.
func buildContainerDefsJSON(
	spec serviceSpec,
	args *ServicesArgs,
	logGroup *cloudwatch.LogGroup,
	awsRegion string,
) pulumi.StringOutput {
	ecrURL := args.ECRRepositoryURLs[spec.ecrKey]

	// Collect all Pulumi outputs we need to resolve.
	allOutputs := []interface{}{
		ecrURL,
		logGroup.Name,
		args.DatabaseSecretArn,
		args.KafkaSecretArn,
		args.RedisSecretArn,
		args.AuthSecretArn,
	}

	// If this service has deps, we also need the healthgate ECR URL.
	hasGate := len(spec.deps) > 0
	if hasGate {
		if gateURL, ok := args.ECRRepositoryURLs["healthgate"]; ok {
			allOutputs = append(allOutputs, gateURL)
		}
	}

	return pulumi.All(allOutputs...).ApplyT(func(vals []interface{}) (string, error) {
		imageURL := vals[0].(string)
		logGroupName := vals[1].(string)
		dbSecretArn := vals[2].(string)
		kafkaSecretArn := vals[3].(string)
		redisSecretArn := vals[4].(string)
		authSecretArn := vals[5].(string)

		var gateImageURL string
		if hasGate && len(vals) > 6 {
			gateImageURL = vals[6].(string)
		}

		logConfig := logCfg{
			LogDriver: "awslogs",
			Options: map[string]string{
				"awslogs-group":         logGroupName,
				"awslogs-region":        awsRegion,
				"awslogs-stream-prefix": spec.name,
			},
		}

		// Port mappings.
		ports := make([]portMap, len(spec.ports))
		for i, p := range spec.ports {
			ports[i] = portMap{ContainerPort: p, Protocol: "tcp"}
		}

		// Environment variables: service discovery endpoints + runtime config.
		envVars := []envKV{
			{Name: "ENVIRONMENT", Value: args.Environment},
		}

		// Language-specific log level.
		switch spec.lang {
		case "rust":
			envVars = append(envVars, envKV{Name: "RUST_LOG", Value: "info"})
		case "go":
			envVars = append(envVars, envKV{Name: "LOG_LEVEL", Value: "info"})
		case "ts":
			envVars = append(envVars, envKV{Name: "NODE_ENV", Value: "production"})
			envVars = append(envVars, envKV{Name: "LOG_LEVEL", Value: "info"})
		}

		// Cloud Map DNS endpoints for service-to-service discovery.
		for k, v := range serviceEndpoints() {
			envVars = append(envVars, envKV{Name: k, Value: v})
		}

		// Secrets from Secrets Manager (injected by ECS agent at task start).
		secrets := []secretRef{
			{Name: "DATABASE_SECRET", ValueFrom: dbSecretArn},
			{Name: "KAFKA_SECRET", ValueFrom: kafkaSecretArn},
			{Name: "REDIS_SECRET", ValueFrom: redisSecretArn},
			{Name: "AUTH_SECRET", ValueFrom: authSecretArn},
		}

		// Build the container list.
		var containers []containerDef

		// Health-gate init container: polls upstream service endpoints, exits 0
		// when all are reachable. Main container waits for SUCCESS condition.
		if hasGate && gateImageURL != "" {
			gateCmd := buildHealthGateCmd(spec.deps)
			gate := containerDef{
				Name:      "health-gate",
				Image:     gateImageURL + ":latest",
				Essential: false,
				Command:   []string{"sh", "-c", gateCmd},
				LogConfiguration: logCfg{
					LogDriver: "awslogs",
					Options: map[string]string{
						"awslogs-group":         logGroupName,
						"awslogs-region":        awsRegion,
						"awslogs-stream-prefix": spec.name + "-health-gate",
					},
				},
				Environment: []envKV{
					{Name: "ENVIRONMENT", Value: args.Environment},
				},
			}
			containers = append(containers, gate)
		}

		mainDef := containerDef{
			Name:             spec.name,
			Image:            imageURL + ":latest",
			Essential:        true,
			PortMappings:     ports,
			LogConfiguration: logConfig,
			Environment:      envVars,
			Secrets:          secrets,
			HealthCheck: &healthCheck{
				Command:     spec.healthCmd,
				Interval:    30,
				Timeout:     5,
				Retries:     3,
				StartPeriod: 60,
			},
		}

		// Chain main container to health-gate via container-level dependsOn.
		if hasGate && gateImageURL != "" {
			mainDef.DependsOn = []containerDependency{
				{ContainerName: "health-gate", Condition: "SUCCESS"},
			}
		}

		containers = append(containers, mainDef)

		b, err := json.Marshal(containers)
		if err != nil {
			return "", fmt.Errorf("marshaling container defs for %s: %w", spec.name, err)
		}
		return string(b), nil
	}).(pulumi.StringOutput)
}

// buildHealthGateCmd generates a shell script that polls each upstream
// dependency endpoint until reachable. Returns a single-line sh -c argument.
//
// HTTP deps are checked via wget (validates the health endpoint returns 2xx).
// TCP deps are checked via nc (validates the port accepts connections).
func buildHealthGateCmd(deps []healthDep) string {
	var b strings.Builder
	b.WriteString("set -e; ")
	for _, dep := range deps {
		switch dep.proto {
		case "http":
			fmt.Fprintf(&b,
				`echo "Waiting for %s at %s:%d%s..."; `+
					`until wget -q --spider -T 2 http://%s:%d%s 2>/dev/null; do sleep 5; done; `+
					`echo "%s ready"; `,
				dep.name, dep.host, dep.port, dep.path,
				dep.host, dep.port, dep.path,
				dep.name,
			)
		default: // "tcp" — gRPC or other TCP-based services
			fmt.Fprintf(&b,
				`echo "Waiting for %s at %s:%d..."; `+
					`until printf "" | nc -w 2 %s %d > /dev/null 2>&1; do sleep 5; done; `+
					`echo "%s ready"; `,
				dep.name, dep.host, dep.port,
				dep.host, dep.port,
				dep.name,
			)
		}
	}
	b.WriteString(`echo "All dependencies healthy"`)
	return b.String()
}

// ---------------------------------------------------------------------------
// IAM roles
// ---------------------------------------------------------------------------

// newExecutionRole creates the ECS task execution role. This role is assumed
// by the ECS agent to pull container images from ECR, push logs to
// CloudWatch, and fetch secrets from Secrets Manager.
func newExecutionRole(ctx *pulumi.Context, prefix string, args *ServicesArgs) (*iam.Role, error) {
	assumeRolePolicy := `{
  "Version": "2012-10-17",
  "Statement": [{
    "Effect": "Allow",
    "Principal": {"Service": "ecs-tasks.amazonaws.com"},
    "Action": "sts:AssumeRole"
  }]
}`

	role, err := iam.NewRole(ctx, "ecs-exec-role", &iam.RoleArgs{
		Name:             pulumi.Sprintf("%s-ecs-exec-role", prefix),
		AssumeRolePolicy: pulumi.String(assumeRolePolicy),
		Tags: pulumi.StringMap{
			"Project":     pulumi.String("kaizen"),
			"Environment": pulumi.String(args.Environment),
			"ManagedBy":   pulumi.String("pulumi"),
		},
	})
	if err != nil {
		return nil, fmt.Errorf("creating execution role: %w", err)
	}

	// Managed policy: ECR pull + CloudWatch Logs.
	_, err = iam.NewRolePolicyAttachment(ctx, "ecs-exec-managed", &iam.RolePolicyAttachmentArgs{
		Role:      role.Name,
		PolicyArn: pulumi.String("arn:aws:iam::aws:policy/service-role/AmazonECSTaskExecutionRolePolicy"),
	})
	if err != nil {
		return nil, fmt.Errorf("attaching execution policy: %w", err)
	}

	// Inline policy: Secrets Manager read access for all Kaizen secrets.
	secretsPolicy := pulumi.All(
		args.DatabaseSecretArn,
		args.KafkaSecretArn,
		args.RedisSecretArn,
		args.AuthSecretArn,
	).ApplyT(func(vals []interface{}) (string, error) {
		arns := make([]string, len(vals))
		for i, v := range vals {
			arns[i] = v.(string)
		}
		policy := map[string]interface{}{
			"Version": "2012-10-17",
			"Statement": []map[string]interface{}{
				{
					"Effect":   "Allow",
					"Action":   []string{"secretsmanager:GetSecretValue"},
					"Resource": arns,
				},
			},
		}
		b, err := json.Marshal(policy)
		return string(b), err
	}).(pulumi.StringOutput)

	_, err = iam.NewRolePolicy(ctx, "ecs-exec-secrets", &iam.RolePolicyArgs{
		Name:   pulumi.Sprintf("%s-ecs-exec-secrets", prefix),
		Role:   role.ID(),
		Policy: secretsPolicy,
	})
	if err != nil {
		return nil, fmt.Errorf("attaching secrets policy: %w", err)
	}

	return role, nil
}

// newTaskRole creates the ECS task role assumed by running containers.
// Grants permissions for Cloud Map discovery, S3 data access (metrics),
// and ECS Exec (SSM for debugging).
func newTaskRole(ctx *pulumi.Context, prefix string) (*iam.Role, error) {
	assumeRolePolicy := `{
  "Version": "2012-10-17",
  "Statement": [{
    "Effect": "Allow",
    "Principal": {"Service": "ecs-tasks.amazonaws.com"},
    "Action": "sts:AssumeRole"
  }]
}`

	role, err := iam.NewRole(ctx, "ecs-task-role", &iam.RoleArgs{
		Name:             pulumi.Sprintf("%s-ecs-task-role", prefix),
		AssumeRolePolicy: pulumi.String(assumeRolePolicy),
		Tags: pulumi.StringMap{
			"Project":   pulumi.String("kaizen"),
			"ManagedBy": pulumi.String("pulumi"),
		},
	})
	if err != nil {
		return nil, fmt.Errorf("creating task role: %w", err)
	}

	// Cloud Map discovery: services can look up other services.
	_, err = iam.NewRolePolicy(ctx, "ecs-task-discovery", &iam.RolePolicyArgs{
		Name: pulumi.Sprintf("%s-task-discovery", prefix),
		Role: role.ID(),
		Policy: pulumi.String(`{
  "Version": "2012-10-17",
  "Statement": [{
    "Effect": "Allow",
    "Action": [
      "servicediscovery:DiscoverInstances",
      "servicediscovery:ListInstances"
    ],
    "Resource": "*"
  }]
}`),
	})
	if err != nil {
		return nil, fmt.Errorf("attaching discovery policy: %w", err)
	}

	// ECS Exec (SSM) for interactive debugging via `aws ecs execute-command`.
	_, err = iam.NewRolePolicyAttachment(ctx, "ecs-task-ssm", &iam.RolePolicyAttachmentArgs{
		Role:      role.Name,
		PolicyArn: pulumi.String("arn:aws:iam::aws:policy/AmazonSSMManagedInstanceCore"),
	})
	if err != nil {
		return nil, fmt.Errorf("attaching SSM policy: %w", err)
	}

	// S3 read/write for metrics data (M3 Delta Lake, MLflow artifacts).
	_, err = iam.NewRolePolicy(ctx, "ecs-task-s3", &iam.RolePolicyArgs{
		Name: pulumi.Sprintf("%s-task-s3", prefix),
		Role: role.ID(),
		Policy: pulumi.Sprintf(`{
  "Version": "2012-10-17",
  "Statement": [{
    "Effect": "Allow",
    "Action": [
      "s3:GetObject",
      "s3:PutObject",
      "s3:ListBucket",
      "s3:DeleteObject"
    ],
    "Resource": [
      "arn:aws:s3:::%s-data",
      "arn:aws:s3:::%s-data/*",
      "arn:aws:s3:::%s-mlflow",
      "arn:aws:s3:::%s-mlflow/*"
    ]
  }]
}`, prefix, prefix, prefix, prefix),
	})
	if err != nil {
		return nil, fmt.Errorf("attaching S3 policy: %w", err)
	}

	return role, nil
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

// logRetentionDays returns the CloudWatch log retention period based on env.
func logRetentionDays(env string) int {
	switch env {
	case "prod":
		return 90
	case "staging":
		return 30
	default:
		return 14
	}
}
