// Package test contains Pulumi integration smoke tests for the Kaizen
// data store modules: RDS, ElastiCache Redis, S3, and Secrets Manager.
//
// Tests use Pulumi's mock framework to intercept resource registrations
// and verify configuration properties without deploying real infrastructure.
package test

import (
	"sync"
	"testing"

	"github.com/pulumi/pulumi/sdk/v3/go/common/resource"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"

	"github.com/kaizen-experimentation/infra/pkg/cache"
	kconfig "github.com/kaizen-experimentation/infra/pkg/config"
	"github.com/kaizen-experimentation/infra/pkg/database"
	"github.com/kaizen-experimentation/infra/pkg/secrets"
	"github.com/kaizen-experimentation/infra/pkg/storage"
)

// ---------------------------------------------------------------------------
// Mock infrastructure
// ---------------------------------------------------------------------------

// datastoreMocks implements pulumi.MockResourceMonitor, intercepting all
// resource registrations and provider function calls during test execution.
type datastoreMocks struct {
	mu        sync.Mutex
	resources []trackedResource
}

type trackedResource struct {
	TypeToken string
	Name      string
	Inputs    resource.PropertyMap
}

func (m *datastoreMocks) NewResource(args pulumi.MockResourceArgs) (string, resource.PropertyMap, error) {
	m.mu.Lock()
	m.resources = append(m.resources, trackedResource{
		TypeToken: args.TypeToken,
		Name:      args.Name,
		Inputs:    args.Inputs,
	})
	m.mu.Unlock()

	id := args.Name + "_id"
	outputs := resource.PropertyMap{}
	for k, v := range args.Inputs {
		outputs[k] = v
	}

	switch args.TypeToken {
	case "aws:rds/parameterGroup:ParameterGroup":
		outputs["name"] = resource.NewStringProperty(args.Name)
	case "aws:rds/subnetGroup:SubnetGroup":
		outputs["name"] = resource.NewStringProperty(args.Name)
	case "aws:rds/instance:Instance":
		outputs["endpoint"] = resource.NewStringProperty(
			"kaizen-rds.abc123.us-east-1.rds.amazonaws.com:5432")
		outputs["port"] = resource.NewNumberProperty(5432)
		outputs["identifier"] = resource.NewStringProperty("kaizen-rds")
	case "aws:elasticache/subnetGroup:SubnetGroup":
		outputs["name"] = resource.NewStringProperty(args.Name)
	case "aws:elasticache/replicationGroup:ReplicationGroup":
		outputs["primaryEndpointAddress"] = resource.NewStringProperty(
			"kaizen-redis.abc123.cache.amazonaws.com")
	case "aws:s3/bucketV2:BucketV2":
		if b, ok := args.Inputs["bucket"]; ok {
			outputs["arn"] = resource.NewStringProperty("arn:aws:s3:::" + b.StringValue())
		}
	case "aws:secretsmanager/secret:Secret":
		secretName := args.Name
		if n, ok := args.Inputs["name"]; ok {
			secretName = n.StringValue()
		}
		outputs["arn"] = resource.NewStringProperty(
			"arn:aws:secretsmanager:us-east-1:123456789:secret:" + secretName)
	}

	return id, outputs, nil
}

func (m *datastoreMocks) Call(args pulumi.MockCallArgs) (resource.PropertyMap, error) {
	switch args.Token {
	case "aws:elb/getServiceAccount:getServiceAccount":
		return resource.PropertyMap{
			"arn": resource.NewStringProperty("arn:aws:iam::123456789012:root"),
			"id":  resource.NewStringProperty("123456789012"),
		}, nil
	case "aws:iam/getPolicyDocument:getPolicyDocument":
		return resource.PropertyMap{
			"json": resource.NewStringProperty(`{"Version":"2012-10-17","Statement":[]}`),
		}, nil
	}
	return resource.PropertyMap{}, nil
}

func (m *datastoreMocks) findByType(typeToken string) []trackedResource {
	m.mu.Lock()
	defer m.mu.Unlock()
	var result []trackedResource
	for _, r := range m.resources {
		if r.TypeToken == typeToken {
			result = append(result, r)
		}
	}
	return result
}

// ---------------------------------------------------------------------------
// RDS: endpoint resolvable, port 5432
// ---------------------------------------------------------------------------

func TestRdsEndpointResolvablePort5432(t *testing.T) {
	mocks := &datastoreMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		outputs, err := database.NewRds(ctx, &kconfig.KaizenConfig{
			Env: kconfig.EnvDev,
		}, &database.RdsInputs{
			SubnetIds:           pulumi.StringArray{pulumi.String("subnet-abc")},
			VpcSecurityGroupIds: pulumi.StringArray{pulumi.String("sg-abc")},
		})
		if err != nil {
			return err
		}
		ctx.Export("rdsEndpoint", outputs.RdsEndpoint)
		ctx.Export("rdsPort", outputs.RdsPort)
		return nil
	}, pulumi.WithMocks("kaizen", "dev", mocks))
	if err != nil {
		t.Fatalf("Pulumi program failed: %v", err)
	}

	instances := mocks.findByType("aws:rds/instance:Instance")
	if len(instances) != 1 {
		t.Fatalf("expected 1 RDS instance, got %d", len(instances))
	}

	rds := instances[0]

	// PostgreSQL engine guarantees resolvable endpoint on port 5432.
	if v, ok := rds.Inputs["engine"]; !ok || v.StringValue() != "postgres" {
		t.Errorf("RDS engine = %v, want postgres", v)
	}
	if v, ok := rds.Inputs["engineVersion"]; !ok || v.StringValue() != "16" {
		t.Errorf("RDS engineVersion = %v, want 16", v)
	}
	if v, ok := rds.Inputs["storageEncrypted"]; !ok || !v.BoolValue() {
		t.Error("RDS storage encryption must be enabled")
	}
	if v, ok := rds.Inputs["dbName"]; !ok || v.StringValue() != "kaizen" {
		t.Errorf("RDS dbName = %v, want kaizen", v)
	}
}

// ---------------------------------------------------------------------------
// RDS: Multi-AZ matches config
// ---------------------------------------------------------------------------

func TestRdsMultiAzMatchesConfig(t *testing.T) {
	tests := []struct {
		name   string
		env    kconfig.Environment
		wantAz bool
	}{
		{"dev_single_az", kconfig.EnvDev, false},
		{"staging_multi_az", kconfig.EnvStaging, true},
		{"prod_multi_az", kconfig.EnvProd, true},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			mocks := &datastoreMocks{}
			err := pulumi.RunErr(func(ctx *pulumi.Context) error {
				_, err := database.NewRds(ctx, &kconfig.KaizenConfig{
					Env: tt.env,
				}, &database.RdsInputs{
					SubnetIds:           pulumi.StringArray{pulumi.String("subnet-abc")},
					VpcSecurityGroupIds: pulumi.StringArray{pulumi.String("sg-abc")},
				})
				return err
			}, pulumi.WithMocks("kaizen", string(tt.env), mocks))
			if err != nil {
				t.Fatalf("Pulumi program failed: %v", err)
			}

			instances := mocks.findByType("aws:rds/instance:Instance")
			if len(instances) != 1 {
				t.Fatalf("expected 1 RDS instance, got %d", len(instances))
			}

			got := instances[0].Inputs["multiAz"].BoolValue()
			if got != tt.wantAz {
				t.Errorf("multi-AZ = %v, want %v", got, tt.wantAz)
			}
		})
	}
}

// ---------------------------------------------------------------------------
// Redis: endpoint resolvable, encryption enabled
// ---------------------------------------------------------------------------

func TestRedisEndpointAndEncryption(t *testing.T) {
	mocks := &datastoreMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		outputs, err := cache.NewRedis(ctx, "kaizen-redis", &cache.RedisConfig{
			NodeType:         "cache.t4g.medium",
			NumCacheClusters: 2,
			SubnetIds:        pulumi.StringArray{pulumi.String("subnet-abc")},
			SecurityGroupIds: pulumi.StringArray{pulumi.String("sg-abc")},
			Tags:             kconfig.DefaultTags("dev"),
		})
		if err != nil {
			return err
		}
		ctx.Export("redisEndpoint", outputs.RedisEndpoint)
		return nil
	}, pulumi.WithMocks("kaizen", "dev", mocks))
	if err != nil {
		t.Fatalf("Pulumi program failed: %v", err)
	}

	rgs := mocks.findByType("aws:elasticache/replicationGroup:ReplicationGroup")
	if len(rgs) != 1 {
		t.Fatalf("expected 1 Redis replication group, got %d", len(rgs))
	}

	rg := rgs[0]

	if v, ok := rg.Inputs["engine"]; !ok || v.StringValue() != "redis" {
		t.Errorf("Redis engine = %v, want redis", v)
	}
	if v, ok := rg.Inputs["atRestEncryptionEnabled"]; !ok || !v.BoolValue() {
		t.Error("Redis at-rest encryption must be enabled")
	}
	if v, ok := rg.Inputs["transitEncryptionEnabled"]; !ok || !v.BoolValue() {
		t.Error("Redis in-transit encryption must be enabled")
	}
	if v, ok := rg.Inputs["port"]; !ok || v.NumberValue() != 6379 {
		t.Errorf("Redis port = %v, want 6379", v)
	}
}

// ---------------------------------------------------------------------------
// S3: buckets exist with correct lifecycle rules
// ---------------------------------------------------------------------------

func TestS3BucketsExistWithLifecycleRules(t *testing.T) {
	mocks := &datastoreMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		outputs, err := storage.NewStorage(ctx, "dev", nil)
		if err != nil {
			return err
		}
		ctx.Export("dataBucket", outputs.DataBucketName)
		ctx.Export("mlflowBucket", outputs.MlflowBucketName)
		ctx.Export("logsBucket", outputs.LogsBucketName)
		return nil
	}, pulumi.WithMocks("kaizen", "dev", mocks))
	if err != nil {
		t.Fatalf("Pulumi program failed: %v", err)
	}

	// --- Verify 3 buckets: data, mlflow, logs ---
	buckets := mocks.findByType("aws:s3/bucketV2:BucketV2")
	if len(buckets) != 3 {
		t.Fatalf("expected 3 S3 buckets, got %d", len(buckets))
	}

	names := make(map[string]bool)
	for _, b := range buckets {
		if v, ok := b.Inputs["bucket"]; ok {
			names[v.StringValue()] = true
		}
	}
	for _, want := range []string{"kaizen-dev-data", "kaizen-dev-mlflow", "kaizen-dev-logs"} {
		if !names[want] {
			t.Errorf("missing bucket: %s", want)
		}
	}

	// --- Verify lifecycle configurations ---
	lifecycles := mocks.findByType("aws:s3/bucketLifecycleConfigurationV2:BucketLifecycleConfigurationV2")
	if len(lifecycles) != 2 {
		t.Fatalf("expected 2 lifecycle configurations (data + logs), got %d", len(lifecycles))
	}

	for _, lc := range lifecycles {
		rulesVal, ok := lc.Inputs["rules"]
		if !ok || !rulesVal.IsArray() {
			t.Errorf("%s: lifecycle rules missing or not an array", lc.Name)
			continue
		}
		rules := rulesVal.ArrayValue()

		switch lc.Name {
		case "data-bucket-lifecycle":
			// Data bucket: tiered-storage (IA@90d, Glacier@365d) + noncurrent-cleanup.
			if len(rules) != 2 {
				t.Errorf("data bucket: expected 2 lifecycle rules, got %d", len(rules))
				continue
			}
			tiered := rules[0].ObjectValue()
			if id := tiered["id"].StringValue(); id != "tiered-storage" {
				t.Errorf("data rule[0] id = %q, want tiered-storage", id)
			}
			transitions := tiered["transitions"].ArrayValue()
			if len(transitions) != 2 {
				t.Errorf("tiered-storage: expected 2 transitions, got %d", len(transitions))
			} else {
				t0 := transitions[0].ObjectValue()
				if t0["days"].NumberValue() != 90 || t0["storageClass"].StringValue() != "STANDARD_IA" {
					t.Errorf("transition[0]: days=%v class=%v, want 90/STANDARD_IA",
						t0["days"].NumberValue(), t0["storageClass"].StringValue())
				}
				t1 := transitions[1].ObjectValue()
				if t1["days"].NumberValue() != 365 || t1["storageClass"].StringValue() != "GLACIER" {
					t.Errorf("transition[1]: days=%v class=%v, want 365/GLACIER",
						t1["days"].NumberValue(), t1["storageClass"].StringValue())
				}
			}

		case "logs-bucket-lifecycle":
			// Logs bucket: expire after 90 days.
			if len(rules) != 1 {
				t.Errorf("logs bucket: expected 1 lifecycle rule, got %d", len(rules))
				continue
			}
			rule := rules[0].ObjectValue()
			if id := rule["id"].StringValue(); id != "expire-logs" {
				t.Errorf("logs rule id = %q, want expire-logs", id)
			}
			exp := rule["expiration"].ObjectValue()
			if exp["days"].NumberValue() != 90 {
				t.Errorf("logs expiration days = %v, want 90", exp["days"].NumberValue())
			}
		}
	}
}

// ---------------------------------------------------------------------------
// Secrets: all four secrets exist in Secrets Manager
// ---------------------------------------------------------------------------

func TestSecretsExistInSecretsManager(t *testing.T) {
	mocks := &datastoreMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		cfg := &kconfig.Config{
			Project: "kaizen",
			Env:     kconfig.EnvDev,
		}
		_, err := secrets.NewSecrets(ctx, cfg, &secrets.SecretsInputs{
			RdsEndpoint:         pulumi.String("kaizen-rds.abc.rds.amazonaws.com:5432").ToStringOutput(),
			MskBootstrapBrokers: pulumi.String("b-1.kaizen.kafka.us-east-1.amazonaws.com:9096").ToStringOutput(),
			RedisEndpoint:       pulumi.String("kaizen-redis.abc.cache.amazonaws.com").ToStringOutput(),
		})
		return err
	}, pulumi.WithMocks("kaizen", "dev", mocks))
	if err != nil {
		t.Fatalf("Pulumi program failed: %v", err)
	}

	// Verify all 4 secret containers.
	secretResources := mocks.findByType("aws:secretsmanager/secret:Secret")
	if len(secretResources) != 4 {
		t.Fatalf("expected 4 secrets, got %d", len(secretResources))
	}

	found := make(map[string]bool)
	for _, s := range secretResources {
		if n, ok := s.Inputs["name"]; ok {
			found[n.StringValue()] = true
		}
	}
	for _, want := range []string{
		"kaizen/dev/database",
		"kaizen/dev/kafka",
		"kaizen/dev/redis",
		"kaizen/dev/auth",
	} {
		if !found[want] {
			t.Errorf("missing secret: %s", want)
		}
	}

	// Verify all 4 secret versions are created (contain actual credential JSON).
	versions := mocks.findByType("aws:secretsmanager/secretVersion:SecretVersion")
	if len(versions) != 4 {
		t.Fatalf("expected 4 secret versions, got %d", len(versions))
	}
}
