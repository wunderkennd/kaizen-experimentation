// Package database provisions RDS PostgreSQL resources for the Kaizen
// experimentation platform. Sprint I.0 creates the module code; Sprint I.1
// wires it to VPC subnets and security groups.
package database

import (
	"fmt"

	"github.com/pulumi/pulumi-aws/sdk/v6/go/aws/rds"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi/config"

	kconfig "github.com/kaizen-experimentation/infra/pkg/config"
)

// DatabaseOutputs holds the outputs consumed by downstream modules (compute,
// secrets). The struct shape is part of the cross-agent contract defined in
// docs/coordination/iac-implementation-plan.md.
type DatabaseOutputs struct {
	RdsEndpoint pulumi.StringOutput
	RdsPort     pulumi.IntOutput
}

// RdsInputs contains optional overrides injected by the caller (main.go) once
// networking is wired in Sprint I.1. Until then, placeholder values are used.
type RdsInputs struct {
	// SubnetIds are the private subnet IDs for the DB subnet group.
	// Wired in Sprint I.1 from NetworkOutputs.PrivateSubnetIds.
	SubnetIds pulumi.StringArrayInput

	// VpcSecurityGroupIds are the security group IDs to attach.
	// Wired in Sprint I.1 from NetworkOutputs.SecurityGroups["rds"].
	VpcSecurityGroupIds pulumi.StringArrayInput
}

// NewRds creates an RDS PostgreSQL 16 instance with a custom parameter group
// and a DB subnet group placeholder. Configuration is read from Pulumi stack
// config under the "kaizen:" namespace.
func NewRds(ctx *pulumi.Context, kcfg *kconfig.KaizenConfig, inputs *RdsInputs) (*DatabaseOutputs, error) {
	cfg := config.New(ctx, "kaizen")

	// --- Instance class ---
	// Default per environment table in iac-implementation-plan.md:
	//   dev: db.t4g.medium, staging/prod: db.r6g.large
	instanceClass := cfg.Get("rdsInstanceClass")
	if instanceClass == "" {
		if kcfg.IsProd() || kcfg.IsStaging() {
			instanceClass = "db.r6g.large"
		} else {
			instanceClass = "db.t4g.medium"
		}
	}

	// --- Multi-AZ ---
	// dev: false, staging/prod: true (overridable via kaizen:rdsMultiAz)
	multiAz := kcfg.IsProd() || kcfg.IsStaging()
	if v, err := cfg.TryBool("rdsMultiAz"); err == nil {
		multiAz = v
	}

	// --- Deletion protection ---
	// Always on in prod; off elsewhere so dev stacks can be torn down.
	deletionProtection := kcfg.IsProd()

	// --- Parameter group (PG 16 tuning) ---
	paramGroup, err := rds.NewParameterGroup(ctx, "kaizen-pg16-params", &rds.ParameterGroupArgs{
		Family:      pulumi.String("postgres16"),
		Description: pulumi.String("Kaizen experimentation platform — PG 16 tuning"),
		Parameters: rds.ParameterGroupParameterArray{
			&rds.ParameterGroupParameterArgs{
				Name:        pulumi.String("shared_buffers"),
				Value:       pulumi.String("524288"), // 4 GB in 8 kB pages
				ApplyMethod: pulumi.String("pending-reboot"),
			},
			&rds.ParameterGroupParameterArgs{
				Name:  pulumi.String("work_mem"),
				Value: pulumi.String("65536"), // 64 MB in kB
			},
			&rds.ParameterGroupParameterArgs{
				Name:        pulumi.String("max_connections"),
				Value:       pulumi.String("200"),
				ApplyMethod: pulumi.String("pending-reboot"),
			},
		},
	})
	if err != nil {
		return nil, fmt.Errorf("creating RDS parameter group: %w", err)
	}

	// --- DB subnet group (placeholder — wired to VPC in Sprint I.1) ---
	var subnetGroupName pulumi.StringInput
	if inputs != nil && inputs.SubnetIds != nil {
		sg, err := rds.NewSubnetGroup(ctx, "kaizen-db-subnets", &rds.SubnetGroupArgs{
			SubnetIds:   inputs.SubnetIds,
			Description: pulumi.String("Kaizen RDS — private subnets"),
			Tags: pulumi.StringMap{
				"Project": pulumi.String("kaizen"),
			},
		})
		if err != nil {
			return nil, fmt.Errorf("creating RDS subnet group: %w", err)
		}
		subnetGroupName = sg.Name
	}

	// --- RDS instance ---
	instanceArgs := &rds.InstanceArgs{
		Engine:               pulumi.String("postgres"),
		EngineVersion:        pulumi.String("16"),
		InstanceClass:        pulumi.String(instanceClass),
		AllocatedStorage:     pulumi.Int(100),
		MaxAllocatedStorage:  pulumi.Int(500),
		StorageType:          pulumi.String("gp3"),
		StorageEncrypted:     pulumi.Bool(true),
		DbName:               pulumi.String("kaizen"),
		Username:             pulumi.String("kaizen_admin"),
		ManageMasterUserPassword: pulumi.Bool(true),
		ParameterGroupName:  paramGroup.Name,
		MultiAz:             pulumi.Bool(multiAz),
		DeletionProtection:  pulumi.Bool(deletionProtection),
		BackupRetentionPeriod: pulumi.Int(7),
		BackupWindow:        pulumi.String("03:00-04:00"),
		MaintenanceWindow:   pulumi.String("sun:05:00-sun:06:00"),
		CopyTagsToSnapshot: pulumi.Bool(true),
		SkipFinalSnapshot:  pulumi.Bool(!kcfg.IsProd()),
		Tags: pulumi.StringMap{
			"Project":     pulumi.String("kaizen"),
			"Environment": pulumi.String(string(kcfg.Env)),
			"ManagedBy":   pulumi.String("pulumi"),
		},
	}

	// Prod requires a final snapshot; non-prod skips it.
	if kcfg.IsProd() {
		instanceArgs.FinalSnapshotIdentifier = pulumi.Sprintf("kaizen-%s-final", string(kcfg.Env))
	}

	// Wire subnet group and security groups when available (Sprint I.1).
	if subnetGroupName != nil {
		instanceArgs.DbSubnetGroupName = subnetGroupName
	}
	if inputs != nil && inputs.VpcSecurityGroupIds != nil {
		instanceArgs.VpcSecurityGroupIds = inputs.VpcSecurityGroupIds
	}

	instance, err := rds.NewInstance(ctx, "kaizen-rds", instanceArgs)
	if err != nil {
		return nil, fmt.Errorf("creating RDS instance: %w", err)
	}

	return &DatabaseOutputs{
		RdsEndpoint: instance.Endpoint,
		RdsPort:     instance.Port,
	}, nil
}
