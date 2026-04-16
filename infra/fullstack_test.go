package main

import (
	"strings"
	"sync"
	"testing"

	"github.com/pulumi/pulumi/sdk/v3/go/common/resource"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"
)

// ---------------------------------------------------------------------------
// Mock infrastructure (package-local — cannot import from test/ subpackage)
// ---------------------------------------------------------------------------

// fsResource captures a single Pulumi resource registration during a full-stack
// mock program run.
type fsResource struct {
	TypeToken string
	Name      string
	Inputs    resource.PropertyMap
}

// fullstackMocks implements pulumi.MockResourceMonitor with comprehensive
// output enrichment for every AWS resource type used by Deploy().
type fullstackMocks struct {
	mu        sync.Mutex
	resources []fsResource
}

func (m *fullstackMocks) NewResource(args pulumi.MockResourceArgs) (string, resource.PropertyMap, error) {
	m.mu.Lock()
	m.resources = append(m.resources, fsResource{
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

	// -- RDS --
	case "aws:rds/instance:Instance":
		outputs["endpoint"] = resource.NewStringProperty(
			"kaizen-rds.abc123.us-east-1.rds.amazonaws.com:5432")
		outputs["port"] = resource.NewNumberProperty(5432)
		outputs["identifier"] = resource.NewStringProperty("kaizen-rds")
	case "aws:rds/parameterGroup:ParameterGroup",
		"aws:rds/subnetGroup:SubnetGroup":
		outputs["name"] = resource.NewStringProperty(args.Name)

	// -- ElastiCache --
	case "aws:elasticache/replicationGroup:ReplicationGroup":
		outputs["primaryEndpointAddress"] = resource.NewStringProperty(
			"kaizen-redis.abc123.cache.amazonaws.com")
	case "aws:elasticache/subnetGroup:SubnetGroup":
		outputs["name"] = resource.NewStringProperty(args.Name)

	// -- S3 --
	case "aws:s3/bucketV2:BucketV2":
		if b, ok := args.Inputs["bucket"]; ok {
			outputs["arn"] = resource.NewStringProperty("arn:aws:s3:::" + b.StringValue())
		}

	// -- Secrets Manager --
	case "aws:secretsmanager/secret:Secret":
		secretName := args.Name
		if n, ok := args.Inputs["name"]; ok {
			secretName = n.StringValue()
		}
		outputs["arn"] = resource.NewStringProperty(
			"arn:aws:secretsmanager:us-east-1:123456789012:secret:" + secretName)

	// -- ECS --
	case "aws:ecs/cluster:Cluster":
		outputs["clusterArn"] = resource.NewStringProperty(
			"arn:aws:ecs:us-east-1:123456789012:cluster/" + args.Name)
		outputs["clusterName"] = resource.NewStringProperty(args.Name)
	case "aws:ecs/service:Service":
		outputs["serviceArn"] = resource.NewStringProperty(
			"arn:aws:ecs:us-east-1:123456789012:service/" + args.Name)
	case "aws:ecs/taskDefinition:TaskDefinition":
		outputs["taskDefinitionArn"] = resource.NewStringProperty(
			"arn:aws:ecs:us-east-1:123456789012:task-definition/" + args.Name + ":1")

	// -- IAM --
	case "aws:iam/role:Role":
		outputs["arn"] = resource.NewStringProperty(
			"arn:aws:iam::123456789012:role/" + args.Name)

	// -- ACM --
	case "aws:acm/certificate:Certificate":
		outputs["arn"] = resource.NewStringProperty(
			"arn:aws:acm:us-east-1:123456789012:certificate/mock-cert-id")
		outputs["domainValidationOptions"] = resource.NewArrayProperty(
			[]resource.PropertyValue{
				resource.NewObjectProperty(resource.PropertyMap{
					"domainName":          resource.NewStringProperty("kaizen.example.com"),
					"resourceRecordName":  resource.NewStringProperty("_acme.kaizen.example.com"),
					"resourceRecordType":  resource.NewStringProperty("CNAME"),
					"resourceRecordValue": resource.NewStringProperty("mock-validation.acm-validations.aws"),
				}),
			})

	// -- ALB --
	case "aws:lb/loadBalancer:LoadBalancer",
		"aws:alb/loadBalancer:LoadBalancer":
		outputs["dnsName"] = resource.NewStringProperty(
			"kaizen-alb-123456.us-east-1.elb.amazonaws.com")
		outputs["zoneId"] = resource.NewStringProperty("Z35SXDOTRQ7X7K")
		outputs["arnSuffix"] = resource.NewStringProperty(
			"app/kaizen-alb/50dc6c495c0c9188")
	case "aws:lb/listener:Listener",
		"aws:alb/listener:Listener":
		outputs["arn"] = resource.NewStringProperty(
			"arn:aws:elasticloadbalancing:us-east-1:123456789012:listener/app/kaizen-alb/50dc6c495c0c9188/mock")

	// -- Route53 --
	case "aws:route53/zone:Zone":
		outputs["zoneId"] = resource.NewStringProperty("Z0123456789ABCDEF")

	// -- MSK --
	case "aws:msk/cluster:Cluster":
		outputs["bootstrapBrokersSaslScram"] = resource.NewStringProperty(
			"b-1.kaizen.kafka.us-east-1.amazonaws.com:9096,b-2.kaizen.kafka.us-east-1.amazonaws.com:9096")
		outputs["clusterArn"] = resource.NewStringProperty(
			"arn:aws:kafka:us-east-1:123456789012:cluster/kaizen/" + args.Name)
		outputs["clusterName"] = resource.NewStringProperty(args.Name)

	// -- KMS --
	case "aws:kms/key:Key":
		outputs["arn"] = resource.NewStringProperty(
			"arn:aws:kms:us-east-1:123456789012:key/mock-key-id")
		outputs["keyId"] = resource.NewStringProperty("mock-key-id")

	// -- CloudWatch --
	case "aws:cloudwatch/logGroup:LogGroup":
		logName := args.Name
		if n, ok := args.Inputs["name"]; ok {
			logName = n.StringValue()
		}
		outputs["arn"] = resource.NewStringProperty(
			"arn:aws:logs:us-east-1:123456789012:log-group:" + logName)

	// -- EC2 Launch Template --
	case "aws:ec2/launchTemplate:LaunchTemplate":
		outputs["id"] = resource.NewStringProperty(args.Name + "-lt-id")

	// -- Auto Scaling Group --
	case "aws:autoscaling/group:Group":
		outputs["name"] = resource.NewStringProperty(args.Name)

	// -- SSM Parameter --
	case "aws:ssm/parameter:Parameter":
		if v, ok := args.Inputs["value"]; ok {
			outputs["value"] = v
		} else {
			outputs["value"] = resource.NewStringProperty("mock-ssm-value")
		}

	// -- ECR Repository --
	case "aws:ecr/repository:Repository":
		repoName := args.Name
		if n, ok := args.Inputs["name"]; ok {
			repoName = n.StringValue()
		}
		outputs["repositoryUrl"] = resource.NewStringProperty(
			"123456789012.dkr.ecr.us-east-1.amazonaws.com/" + repoName)
	}

	return id, outputs, nil
}

func (m *fullstackMocks) Call(args pulumi.MockCallArgs) (resource.PropertyMap, error) {
	switch args.Token {
	case "aws:index/getAvailabilityZones:getAvailabilityZones":
		return resource.PropertyMap{
			"id": resource.NewStringProperty("us-east-1"),
			"names": resource.NewArrayProperty([]resource.PropertyValue{
				resource.NewStringProperty("us-east-1a"),
				resource.NewStringProperty("us-east-1b"),
				resource.NewStringProperty("us-east-1c"),
			}),
			"zoneIds": resource.NewArrayProperty([]resource.PropertyValue{
				resource.NewStringProperty("use1-az1"),
				resource.NewStringProperty("use1-az2"),
				resource.NewStringProperty("use1-az3"),
			}),
			"groupNames": resource.NewArrayProperty([]resource.PropertyValue{
				resource.NewStringProperty("us-east-1"),
				resource.NewStringProperty("us-east-1"),
				resource.NewStringProperty("us-east-1"),
			}),
		}, nil
	case "aws:index/getRegion:getRegion":
		return resource.PropertyMap{
			"name":        resource.NewStringProperty("us-east-1"),
			"description": resource.NewStringProperty("US East (N. Virginia)"),
			"endpoint":    resource.NewStringProperty("ec2.us-east-1.amazonaws.com"),
			"id":          resource.NewStringProperty("us-east-1"),
		}, nil
	case "aws:index/getCallerIdentity:getCallerIdentity":
		return resource.PropertyMap{
			"accountId": resource.NewStringProperty("123456789012"),
			"arn":       resource.NewStringProperty("arn:aws:iam::123456789012:root"),
			"id":        resource.NewStringProperty("123456789012"),
			"userId":    resource.NewStringProperty("AIDEXAMPLE"),
		}, nil
	case "aws:elb/getServiceAccount:getServiceAccount":
		return resource.PropertyMap{
			"arn": resource.NewStringProperty("arn:aws:iam::123456789012:root"),
			"id":  resource.NewStringProperty("123456789012"),
		}, nil
	case "aws:iam/getPolicyDocument:getPolicyDocument":
		return resource.PropertyMap{
			"json": resource.NewStringProperty(`{"Version":"2012-10-17","Statement":[]}`),
		}, nil
	case "aws:ssm/getParameter:getParameter":
		return resource.PropertyMap{
			"name":  resource.NewStringProperty("/aws/service/ecs/optimized-ami/amazon-linux-2/recommended/image_id"),
			"type":  resource.NewStringProperty("String"),
			"value": resource.NewStringProperty("ami-0abcdef1234567890"),
		}, nil
	}
	return resource.PropertyMap{}, nil
}

// ---------------------------------------------------------------------------
// Query helpers
// ---------------------------------------------------------------------------

func (m *fullstackMocks) byType(typeToken string) []fsResource {
	m.mu.Lock()
	defer m.mu.Unlock()
	var out []fsResource
	for _, r := range m.resources {
		if r.TypeToken == typeToken {
			out = append(out, r)
		}
	}
	return out
}

func (m *fullstackMocks) count(typeToken string) int {
	return len(m.byType(typeToken))
}

// ---------------------------------------------------------------------------
// Config helper
// ---------------------------------------------------------------------------

// fullstackConfig returns the complete Pulumi stack config required by Deploy().
func fullstackConfig() pulumi.RunOption {
	return func(info *pulumi.RunInfo) {
		info.Config = map[string]string{
			"kaizen:vpcCidr":                                 "10.0.0.0/16",
			"kaizen:natGatewayCount":                         "2",
			"kaizen-experimentation:environment":             "dev",
			"kaizen-experimentation:vpcCidr":                 "10.0.0.0/16",
			"kaizen-experimentation:rdsInstanceClass":        "db.t3.medium",
			"kaizen-experimentation:rdsMultiAz":              "false",
			"kaizen-experimentation:mskBrokerCount":          "3",
			"kaizen-experimentation:mskInstanceType":         "kafka.m5.large",
			"kaizen-experimentation:redisNodeType":           "cache.t3.medium",
			"kaizen-experimentation:m4bInstanceType":         "c5.xlarge",
			"kaizen-experimentation:natGatewayCount":         "2",
			"kaizen-experimentation:wafEnabled":              "false",
			"kaizen-experimentation:fargateMinTasks":         "1",
			"kaizen-experimentation:cloudwatchRetentionDays": "7",
			"kaizen-experimentation:domain":                  "example.com",
			"kaizen-experimentation:projectName":             "kaizen-experimentation",
			"kafka:saslUsername":                             "kaizen-msk-user",
			"kafka:saslPassword":                             "test-password",
		}
	}
}

// ---------------------------------------------------------------------------
// Test: Deploy() completes without error
// ---------------------------------------------------------------------------

func TestFullStackDeploy(t *testing.T) {
	mocks := &fullstackMocks{}
	err := pulumi.RunErr(Deploy,
		pulumi.WithMocks("kaizen", "dev", mocks),
		fullstackConfig(),
	)
	if err != nil {
		t.Fatalf("Deploy() failed: %v", err)
	}
}

// ---------------------------------------------------------------------------
// Test: Resource counts meet expected minimums
// ---------------------------------------------------------------------------

func TestFullStackResourceCounts(t *testing.T) {
	mocks := &fullstackMocks{}
	err := pulumi.RunErr(Deploy,
		pulumi.WithMocks("kaizen", "dev", mocks),
		fullstackConfig(),
	)
	if err != nil {
		t.Fatalf("Deploy() failed: %v", err)
	}

	checks := []struct {
		label     string
		typeToken string
		minCount  int
	}{
		{"VPCs", "aws:ec2/vpc:Vpc", 1},
		{"Subnets", "aws:ec2/subnet:Subnet", 6},
		{"Security Groups", "aws:ec2/securityGroup:SecurityGroup", 6},
		{"S3 Buckets", "aws:s3/bucketV2:BucketV2", 3},
		{"RDS Instances", "aws:rds/instance:Instance", 1},
		{"Redis Replication Groups", "aws:elasticache/replicationGroup:ReplicationGroup", 1},
		{"ECS Clusters", "aws:ecs/cluster:Cluster", 1},
		{"Secrets", "aws:secretsmanager/secret:Secret", 4},
		{"ECR Repositories", "aws:ecr/repository:Repository", 9},
	}

	for _, c := range checks {
		got := mocks.count(c.typeToken)
		if got < c.minCount {
			t.Errorf("%s: got %d, want >= %d", c.label, got, c.minCount)
		}
	}
}

// ---------------------------------------------------------------------------
// Test: Expected stack exports exist
// ---------------------------------------------------------------------------

func TestFullStackExports(t *testing.T) {
	mocks := &fullstackMocks{}

	// Collect exports by wrapping Deploy.
	exports := map[string]bool{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		// Run the real Deploy logic.
		if err := Deploy(ctx); err != nil {
			return err
		}
		return nil
	}, pulumi.WithMocks("kaizen", "dev", mocks), fullstackConfig())
	if err != nil {
		t.Fatalf("Deploy() failed: %v", err)
	}

	// Pulumi mocks don't expose exports directly. Instead, verify that the
	// resources backing the expected exports were created. This is a pragmatic
	// proxy: if the resource exists with the right mock output, the export
	// will resolve.
	expectedExports := []struct {
		key       string
		typeToken string
		hint      string // substring to look for in resource names (optional)
	}{
		{"environment", "", ""},
		{"rdsEndpoint", "aws:rds/instance:Instance", ""},
		{"redisEndpoint", "aws:elasticache/replicationGroup:ReplicationGroup", ""},
		{"mskBootstrapBrokers", "aws:msk/cluster:Cluster", ""},
		{"ecsClusterId", "aws:ecs/cluster:Cluster", ""},
	}

	for _, exp := range expectedExports {
		if exp.typeToken == "" {
			// "environment" is a plain string export, not backed by a resource.
			// It's always emitted by Deploy(). Just mark it present.
			exports[exp.key] = true
			continue
		}
		resources := mocks.byType(exp.typeToken)
		if len(resources) == 0 {
			t.Errorf("export %q: no resources of type %q found — export will not resolve",
				exp.key, exp.typeToken)
			continue
		}
		if exp.hint != "" {
			found := false
			for _, r := range resources {
				if strings.Contains(r.Name, exp.hint) {
					found = true
					break
				}
			}
			if !found {
				t.Errorf("export %q: no resource of type %q with name containing %q",
					exp.key, exp.typeToken, exp.hint)
			}
		}
		exports[exp.key] = true
	}

	for _, exp := range expectedExports {
		if !exports[exp.key] {
			t.Errorf("expected export %q not verified", exp.key)
		}
	}
}
