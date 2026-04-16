// Package test provides shared test utilities for Pulumi mock-based
// infrastructure tests. The universalMocks struct consolidates mock patterns
// from network_test.go and datastore_test.go into a single comprehensive
// implementation that handles all AWS resource types used across the project.
package test

import (
	"sync"

	"github.com/pulumi/pulumi/sdk/v3/go/common/resource"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"
)

// ---------------------------------------------------------------------------
// Universal mock infrastructure
// ---------------------------------------------------------------------------

// universalMocks implements pulumi.MockResourceMonitor and handles ALL AWS
// resource types used across the Kaizen experimentation platform. It records
// every resource in a thread-safe slice and enriches outputs with type-specific
// mock values so downstream references resolve correctly.
type universalMocks struct {
	mu        sync.Mutex
	resources []trackedResource
}

func (m *universalMocks) NewResource(args pulumi.MockResourceArgs) (string, resource.PropertyMap, error) {
	m.mu.Lock()
	m.resources = append(m.resources, trackedResource{
		TypeToken: args.TypeToken,
		Name:      args.Name,
		Inputs:    args.Inputs,
	})
	m.mu.Unlock()

	id := args.Name + "_id"

	// Start with a copy of inputs as the output baseline.
	outputs := resource.PropertyMap{}
	for k, v := range args.Inputs {
		outputs[k] = v
	}

	// Type-specific output enrichment — provides the mock values that
	// downstream resources and exports depend on.
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
					"domainName":       resource.NewStringProperty("kaizen.example.com"),
					"resourceRecordName": resource.NewStringProperty("_acme.kaizen.example.com"),
					"resourceRecordType": resource.NewStringProperty("CNAME"),
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

	// -- Default: inputs copied above are sufficient --
	}

	return id, outputs, nil
}

func (m *universalMocks) Call(args pulumi.MockCallArgs) (resource.PropertyMap, error) {
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

// byType filters tracked resources to those matching the given Pulumi type token.
func (m *universalMocks) byType(typeToken string) []trackedResource {
	m.mu.Lock()
	defer m.mu.Unlock()
	var out []trackedResource
	for _, r := range m.resources {
		if r.TypeToken == typeToken {
			out = append(out, r)
		}
	}
	return out
}

// byName returns tracked resources whose logical name contains the substring.
func (m *universalMocks) byName(substr string) []trackedResource {
	m.mu.Lock()
	defer m.mu.Unlock()
	var out []trackedResource
	for _, r := range m.resources {
		if contains(r.Name, substr) {
			out = append(out, r)
		}
	}
	return out
}

// count returns the number of resources matching the given type token.
func (m *universalMocks) count(typeToken string) int {
	return len(m.byType(typeToken))
}

// contains checks if s contains substr. Placed here to avoid importing
// strings in this file (network_test.go already imports it in the same package).
func contains(s, substr string) bool {
	return len(substr) == 0 || len(s) >= len(substr) && searchSubstr(s, substr)
}

func searchSubstr(s, substr string) bool {
	for i := 0; i <= len(s)-len(substr); i++ {
		if s[i:i+len(substr)] == substr {
			return true
		}
	}
	return false
}

// ---------------------------------------------------------------------------
// Full-stack configuration
// ---------------------------------------------------------------------------

// fullConfig returns the complete set of config keys required by Deploy().
// This is a superset of defaultConfig() — it includes domain, projectName,
// and Kafka credentials that Deploy() requires but module-level tests skip.
func fullConfig() map[string]string {
	return map[string]string{
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
