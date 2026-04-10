// Package test contains Pulumi integration tests for the networking module.
//
// These tests use Pulumi's mock framework to verify that the network module
// creates the expected resources with correct configuration — without
// provisioning real AWS infrastructure.
package test

import (
	"encoding/json"
	"strings"
	"sync"
	"testing"

	"github.com/kaizen-experimentation/infra/pkg/network"
	"github.com/pulumi/pulumi/sdk/v3/go/common/resource"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"
)

// ---------------------------------------------------------------------------
// Mock infrastructure
// ---------------------------------------------------------------------------

// resourceRecord captures a single Pulumi resource registration during a
// mocked program run.
type resourceRecord struct {
	TypeToken string
	Name      string
	Inputs    resource.PropertyMap
}

// networkMocks implements pulumi.MockResourceMonitor, recording every resource
// and handling AWS data-source calls with canned responses.
type networkMocks struct {
	mu        sync.Mutex
	resources []resourceRecord
}

func (m *networkMocks) NewResource(args pulumi.MockResourceArgs) (string, resource.PropertyMap, error) {
	m.mu.Lock()
	defer m.mu.Unlock()
	m.resources = append(m.resources, resourceRecord{
		TypeToken: args.TypeToken,
		Name:      args.Name,
		Inputs:    args.Inputs,
	})
	return args.Name + "-id", args.Inputs, nil
}

func (m *networkMocks) Call(args pulumi.MockCallArgs) (resource.PropertyMap, error) {
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
	}
	return resource.PropertyMap{}, nil
}

// byType filters the collected resources to those matching a Pulumi type token.
func (m *networkMocks) byType(typeToken string) []resourceRecord {
	m.mu.Lock()
	defer m.mu.Unlock()
	var out []resourceRecord
	for _, r := range m.resources {
		if r.TypeToken == typeToken {
			out = append(out, r)
		}
	}
	return out
}

// byName returns resources whose logical name contains the substring.
func (m *networkMocks) byName(substr string) []resourceRecord {
	m.mu.Lock()
	defer m.mu.Unlock()
	var out []resourceRecord
	for _, r := range m.resources {
		if strings.Contains(r.Name, substr) {
			out = append(out, r)
		}
	}
	return out
}

// ---------------------------------------------------------------------------
// Config helper — sets Pulumi stack config for mock tests.
// ---------------------------------------------------------------------------

// withConfig returns a RunOption that injects configuration values.
// Keys use the "namespace:key" format (e.g. "kaizen:vpcCidr").
func withConfig(cfg map[string]string) pulumi.RunOption {
	return func(info *pulumi.RunInfo) {
		info.Config = cfg
	}
}

// defaultConfig returns the standard config map used across all network tests.
func defaultConfig() map[string]string {
	return map[string]string{
		"kaizen:vpcCidr":                        "10.0.0.0/16",
		"kaizen:natGatewayCount":                "2",
		"kaizen-experimentation:environment":    "dev",
		"kaizen-experimentation:vpcCidr":        "10.0.0.0/16",
		"kaizen-experimentation:rdsInstanceClass":  "db.t3.medium",
		"kaizen-experimentation:rdsMultiAz":        "false",
		"kaizen-experimentation:mskBrokerCount":    "3",
		"kaizen-experimentation:mskInstanceType":   "kafka.m5.large",
		"kaizen-experimentation:redisNodeType":     "cache.t3.medium",
		"kaizen-experimentation:m4bInstanceType":   "c5.xlarge",
		"kaizen-experimentation:natGatewayCount":   "2",
		"kaizen-experimentation:wafEnabled":        "false",
		"kaizen-experimentation:fargateMinTasks":   "1",
		"kaizen-experimentation:cloudwatchRetentionDays": "7",
	}
}

// ---------------------------------------------------------------------------
// Pulumi resource type tokens (AWS provider v6)
// ---------------------------------------------------------------------------

const (
	typeVpc              = "aws:ec2/vpc:Vpc"
	typeSubnet           = "aws:ec2/subnet:Subnet"
	typeSecurityGroup    = "aws:ec2/securityGroup:SecurityGroup"
	typeSGRule           = "aws:ec2/securityGroupRule:SecurityGroupRule"
	typeVpcEndpoint      = "aws:ec2/vpcEndpoint:VpcEndpoint"
	typePrivateDnsNS     = "aws:servicediscovery/privateDnsNamespace:PrivateDnsNamespace"
	typeIAMRole          = "aws:iam/role:Role"
	typeIAMRolePolicy    = "aws:iam/rolePolicy:RolePolicy"
	typeIAMAttachment    = "aws:iam/rolePolicyAttachment:RolePolicyAttachment"
	typeOIDCProvider     = "aws:iam/openIdConnectProvider:OpenIdConnectProvider"
)

// ---------------------------------------------------------------------------
// Test: VPC CIDR matches config
// ---------------------------------------------------------------------------

func TestVpcCidrMatchesConfig(t *testing.T) {
	mocks := &networkMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		_, err := network.NewVpc(ctx)
		return err
	}, pulumi.WithMocks("kaizen", "dev", mocks), withConfig(defaultConfig()))
	if err != nil {
		t.Fatalf("pulumi.RunErr: %v", err)
	}

	vpcs := mocks.byType(typeVpc)
	if len(vpcs) != 1 {
		t.Fatalf("expected 1 VPC, got %d", len(vpcs))
	}

	cidr := vpcs[0].Inputs["cidrBlock"].StringValue()
	if cidr != "10.0.0.0/16" {
		t.Errorf("VPC CIDR = %q, want %q", cidr, "10.0.0.0/16")
	}
}

func TestVpcCidrCustom(t *testing.T) {
	mocks := &networkMocks{}
	cfg := defaultConfig()
	cfg["kaizen:vpcCidr"] = "172.16.0.0/16"
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		_, err := network.NewVpc(ctx)
		return err
	}, pulumi.WithMocks("kaizen", "dev", mocks), withConfig(cfg))
	if err != nil {
		t.Fatalf("pulumi.RunErr: %v", err)
	}

	vpcs := mocks.byType(typeVpc)
	if len(vpcs) != 1 {
		t.Fatalf("expected 1 VPC, got %d", len(vpcs))
	}

	cidr := vpcs[0].Inputs["cidrBlock"].StringValue()
	if cidr != "172.16.0.0/16" {
		t.Errorf("VPC CIDR = %q, want %q", cidr, "172.16.0.0/16")
	}
}

// ---------------------------------------------------------------------------
// Test: 3 public + 3 private subnets
// ---------------------------------------------------------------------------

func TestSubnetCounts(t *testing.T) {
	mocks := &networkMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		_, err := network.NewVpc(ctx)
		return err
	}, pulumi.WithMocks("kaizen", "dev", mocks), withConfig(defaultConfig()))
	if err != nil {
		t.Fatalf("pulumi.RunErr: %v", err)
	}

	subnets := mocks.byType(typeSubnet)
	publicCount, privateCount := 0, 0
	for _, s := range subnets {
		if strings.Contains(s.Name, "public") {
			publicCount++
		} else if strings.Contains(s.Name, "private") {
			privateCount++
		}
	}

	if publicCount != 3 {
		t.Errorf("public subnets = %d, want 3", publicCount)
	}
	if privateCount != 3 {
		t.Errorf("private subnets = %d, want 3", privateCount)
	}
}

func TestPublicSubnetsArePublic(t *testing.T) {
	mocks := &networkMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		_, err := network.NewVpc(ctx)
		return err
	}, pulumi.WithMocks("kaizen", "dev", mocks), withConfig(defaultConfig()))
	if err != nil {
		t.Fatalf("pulumi.RunErr: %v", err)
	}

	for _, s := range mocks.byType(typeSubnet) {
		if !strings.Contains(s.Name, "public") {
			continue
		}
		mapPublic, ok := s.Inputs["mapPublicIpOnLaunch"]
		if !ok || !mapPublic.BoolValue() {
			t.Errorf("public subnet %q: mapPublicIpOnLaunch should be true", s.Name)
		}
	}
}

func TestPrivateSubnetsArePrivate(t *testing.T) {
	mocks := &networkMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		_, err := network.NewVpc(ctx)
		return err
	}, pulumi.WithMocks("kaizen", "dev", mocks), withConfig(defaultConfig()))
	if err != nil {
		t.Fatalf("pulumi.RunErr: %v", err)
	}

	for _, s := range mocks.byType(typeSubnet) {
		if !strings.Contains(s.Name, "private") {
			continue
		}
		if mapPublic, ok := s.Inputs["mapPublicIpOnLaunch"]; ok && mapPublic.BoolValue() {
			t.Errorf("private subnet %q: mapPublicIpOnLaunch should not be true", s.Name)
		}
	}
}

func TestSubnetsSpanThreeAZs(t *testing.T) {
	mocks := &networkMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		_, err := network.NewVpc(ctx)
		return err
	}, pulumi.WithMocks("kaizen", "dev", mocks), withConfig(defaultConfig()))
	if err != nil {
		t.Fatalf("pulumi.RunErr: %v", err)
	}

	publicAZs := map[string]bool{}
	privateAZs := map[string]bool{}
	for _, s := range mocks.byType(typeSubnet) {
		az := s.Inputs["availabilityZone"].StringValue()
		if strings.Contains(s.Name, "public") {
			publicAZs[az] = true
		} else if strings.Contains(s.Name, "private") {
			privateAZs[az] = true
		}
	}

	if len(publicAZs) != 3 {
		t.Errorf("public subnets span %d AZs, want 3", len(publicAZs))
	}
	if len(privateAZs) != 3 {
		t.Errorf("private subnets span %d AZs, want 3", len(privateAZs))
	}
}

// ---------------------------------------------------------------------------
// Test: Security group rules are least-privilege (no 0.0.0.0/0 on internal)
// ---------------------------------------------------------------------------

func TestSecurityGroupsCreated(t *testing.T) {
	mocks := &networkMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		vpcOut, err := network.NewVpc(ctx)
		if err != nil {
			return err
		}
		_, err = network.NewSecurityGroups(ctx, "kaizen", &network.SecurityGroupsArgs{
			VpcId: vpcOut.VpcId,
		})
		return err
	}, pulumi.WithMocks("kaizen", "dev", mocks), withConfig(defaultConfig()))
	if err != nil {
		t.Fatalf("pulumi.RunErr: %v", err)
	}

	sgs := mocks.byType(typeSecurityGroup)
	expectedNames := map[string]bool{
		"kaizen-alb-sg":   false,
		"kaizen-ecs-sg":   false,
		"kaizen-rds-sg":   false,
		"kaizen-msk-sg":   false,
		"kaizen-redis-sg": false,
		"kaizen-m4b-sg":   false,
	}

	for _, sg := range sgs {
		if _, ok := expectedNames[sg.Name]; ok {
			expectedNames[sg.Name] = true
		}
	}

	for name, found := range expectedNames {
		if !found {
			t.Errorf("security group %q not created", name)
		}
	}
}

func TestNoOpenCidrOnInternalSecurityGroups(t *testing.T) {
	mocks := &networkMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		vpcOut, err := network.NewVpc(ctx)
		if err != nil {
			return err
		}
		_, err = network.NewSecurityGroups(ctx, "kaizen", &network.SecurityGroupsArgs{
			VpcId: vpcOut.VpcId,
		})
		return err
	}, pulumi.WithMocks("kaizen", "dev", mocks), withConfig(defaultConfig()))
	if err != nil {
		t.Fatalf("pulumi.RunErr: %v", err)
	}

	// Internal security groups must never have 0.0.0.0/0 CIDR rules.
	// Only the ALB ingress rule is allowed to accept internet traffic.
	internalPrefixes := []string{"-ecs-", "-rds-", "-msk-", "-redis-", "-m4b-"}

	for _, rule := range mocks.byType(typeSGRule) {
		cidrs, ok := rule.Inputs["cidrBlocks"]
		if !ok || !cidrs.IsArray() {
			continue
		}

		for _, cidr := range cidrs.ArrayValue() {
			if cidr.StringValue() != "0.0.0.0/0" {
				continue
			}

			// This rule has 0.0.0.0/0 — verify it belongs to the ALB, not internal groups.
			for _, prefix := range internalPrefixes {
				if strings.Contains(rule.Name, prefix) {
					t.Errorf("VIOLATION: rule %q has 0.0.0.0/0 on an internal security group", rule.Name)
				}
			}
		}
	}
}

func TestAlbIngressAllowsHttps(t *testing.T) {
	mocks := &networkMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		vpcOut, err := network.NewVpc(ctx)
		if err != nil {
			return err
		}
		_, err = network.NewSecurityGroups(ctx, "kaizen", &network.SecurityGroupsArgs{
			VpcId: vpcOut.VpcId,
		})
		return err
	}, pulumi.WithMocks("kaizen", "dev", mocks), withConfig(defaultConfig()))
	if err != nil {
		t.Fatalf("pulumi.RunErr: %v", err)
	}

	found := false
	for _, rule := range mocks.byType(typeSGRule) {
		if !strings.Contains(rule.Name, "alb-in-https") {
			continue
		}
		found = true

		ruleType := rule.Inputs["type"].StringValue()
		if ruleType != "ingress" {
			t.Errorf("ALB HTTPS rule type = %q, want ingress", ruleType)
		}

		fromPort := rule.Inputs["fromPort"].NumberValue()
		toPort := rule.Inputs["toPort"].NumberValue()
		if fromPort != 443 || toPort != 443 {
			t.Errorf("ALB HTTPS ports = %v-%v, want 443-443", fromPort, toPort)
		}
	}

	if !found {
		t.Error("ALB HTTPS ingress rule not found")
	}
}

func TestDataStoreRulesArePortRestricted(t *testing.T) {
	mocks := &networkMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		vpcOut, err := network.NewVpc(ctx)
		if err != nil {
			return err
		}
		_, err = network.NewSecurityGroups(ctx, "kaizen", &network.SecurityGroupsArgs{
			VpcId: vpcOut.VpcId,
		})
		return err
	}, pulumi.WithMocks("kaizen", "dev", mocks), withConfig(defaultConfig()))
	if err != nil {
		t.Fatalf("pulumi.RunErr: %v", err)
	}

	// Data store ingress rules must be restricted to specific ports:
	//   RDS: 5432, MSK: 9092/9094/9096, Redis: 6379
	expectedPorts := map[string][]float64{
		"rds-in":   {5432},
		"msk-in":   {9092, 9094, 9096},
		"redis-in": {6379},
	}

	for _, rule := range mocks.byType(typeSGRule) {
		ruleType := rule.Inputs["type"].StringValue()
		if ruleType != "ingress" {
			continue
		}

		for prefix, ports := range expectedPorts {
			if !strings.Contains(rule.Name, prefix) {
				continue
			}

			fromPort := rule.Inputs["fromPort"].NumberValue()
			toPort := rule.Inputs["toPort"].NumberValue()

			// fromPort must equal toPort (single port rule, not a range).
			if fromPort != toPort {
				t.Errorf("rule %q: port range %v-%v should be a single port", rule.Name, fromPort, toPort)
			}

			portAllowed := false
			for _, p := range ports {
				if fromPort == p {
					portAllowed = true
					break
				}
			}
			if !portAllowed {
				t.Errorf("rule %q: port %v not in allowed set %v", rule.Name, fromPort, ports)
			}
		}
	}
}

// ---------------------------------------------------------------------------
// Test: Cloud Map namespace exists
// ---------------------------------------------------------------------------

func TestCloudMapNamespaceExists(t *testing.T) {
	mocks := &networkMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		vpcOut, err := network.NewVpc(ctx)
		if err != nil {
			return err
		}
		_, err = network.NewServiceDiscovery(ctx, &network.ServiceDiscoveryArgs{
			VpcId: vpcOut.VpcId,
		})
		return err
	}, pulumi.WithMocks("kaizen", "dev", mocks), withConfig(defaultConfig()))
	if err != nil {
		t.Fatalf("pulumi.RunErr: %v", err)
	}

	namespaces := mocks.byType(typePrivateDnsNS)
	if len(namespaces) != 1 {
		t.Fatalf("expected 1 Cloud Map namespace, got %d", len(namespaces))
	}

	ns := namespaces[0]
	name := ns.Inputs["name"].StringValue()
	if name != "kaizen.local" {
		t.Errorf("namespace name = %q, want %q", name, "kaizen.local")
	}
}

// ---------------------------------------------------------------------------
// Test: VPC endpoints exist for S3, ECR, CW Logs, Secrets Manager
// ---------------------------------------------------------------------------

func TestVpcEndpointsExist(t *testing.T) {
	mocks := &networkMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		vpcOut, err := network.NewVpc(ctx)
		if err != nil {
			return err
		}
		sgResult, err := network.NewSecurityGroups(ctx, "kaizen", &network.SecurityGroupsArgs{
			VpcId: vpcOut.VpcId,
		})
		if err != nil {
			return err
		}
		_, err = network.NewVpcEndpoints(ctx, &network.VpcEndpointArgs{
			VpcId:                vpcOut.VpcId,
			PrivateSubnetIds:     vpcOut.PrivateSubnetIds,
			PrivateRouteTableIds: vpcOut.PrivateRouteTableIds,
			EcsSecurityGroupId:   sgResult.Groups["ecs"],
			M4bSecurityGroupId:   sgResult.Groups["m4b"],
		})
		return err
	}, pulumi.WithMocks("kaizen", "dev", mocks), withConfig(defaultConfig()))
	if err != nil {
		t.Fatalf("pulumi.RunErr: %v", err)
	}

	endpoints := mocks.byType(typeVpcEndpoint)

	// Expected endpoints: S3 (gateway), ECR DKR, ECR API, CW Logs, Secrets Manager
	expectedServices := map[string]bool{
		"com.amazonaws.us-east-1.s3":             false,
		"com.amazonaws.us-east-1.ecr.dkr":       false,
		"com.amazonaws.us-east-1.ecr.api":       false,
		"com.amazonaws.us-east-1.logs":           false,
		"com.amazonaws.us-east-1.secretsmanager": false,
	}

	for _, ep := range endpoints {
		svc := ep.Inputs["serviceName"].StringValue()
		if _, ok := expectedServices[svc]; ok {
			expectedServices[svc] = true
		}
	}

	for svc, found := range expectedServices {
		if !found {
			t.Errorf("VPC endpoint for %q not created", svc)
		}
	}
}

func TestS3EndpointIsGatewayType(t *testing.T) {
	mocks := &networkMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		vpcOut, err := network.NewVpc(ctx)
		if err != nil {
			return err
		}
		sgResult, err := network.NewSecurityGroups(ctx, "kaizen", &network.SecurityGroupsArgs{
			VpcId: vpcOut.VpcId,
		})
		if err != nil {
			return err
		}
		_, err = network.NewVpcEndpoints(ctx, &network.VpcEndpointArgs{
			VpcId:                vpcOut.VpcId,
			PrivateSubnetIds:     vpcOut.PrivateSubnetIds,
			PrivateRouteTableIds: vpcOut.PrivateRouteTableIds,
			EcsSecurityGroupId:   sgResult.Groups["ecs"],
			M4bSecurityGroupId:   sgResult.Groups["m4b"],
		})
		return err
	}, pulumi.WithMocks("kaizen", "dev", mocks), withConfig(defaultConfig()))
	if err != nil {
		t.Fatalf("pulumi.RunErr: %v", err)
	}

	for _, ep := range mocks.byType(typeVpcEndpoint) {
		svc := ep.Inputs["serviceName"].StringValue()
		epType := ep.Inputs["vpcEndpointType"].StringValue()

		if strings.HasSuffix(svc, ".s3") {
			if epType != "Gateway" {
				t.Errorf("S3 endpoint type = %q, want Gateway", epType)
			}
		} else {
			if epType != "Interface" {
				t.Errorf("endpoint %q type = %q, want Interface", svc, epType)
			}
		}
	}
}

func TestInterfaceEndpointsHavePrivateDns(t *testing.T) {
	mocks := &networkMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		vpcOut, err := network.NewVpc(ctx)
		if err != nil {
			return err
		}
		sgResult, err := network.NewSecurityGroups(ctx, "kaizen", &network.SecurityGroupsArgs{
			VpcId: vpcOut.VpcId,
		})
		if err != nil {
			return err
		}
		_, err = network.NewVpcEndpoints(ctx, &network.VpcEndpointArgs{
			VpcId:                vpcOut.VpcId,
			PrivateSubnetIds:     vpcOut.PrivateSubnetIds,
			PrivateRouteTableIds: vpcOut.PrivateRouteTableIds,
			EcsSecurityGroupId:   sgResult.Groups["ecs"],
			M4bSecurityGroupId:   sgResult.Groups["m4b"],
		})
		return err
	}, pulumi.WithMocks("kaizen", "dev", mocks), withConfig(defaultConfig()))
	if err != nil {
		t.Fatalf("pulumi.RunErr: %v", err)
	}

	for _, ep := range mocks.byType(typeVpcEndpoint) {
		epType := ep.Inputs["vpcEndpointType"].StringValue()
		if epType != "Interface" {
			continue
		}

		privateDns, ok := ep.Inputs["privateDnsEnabled"]
		if !ok || !privateDns.BoolValue() {
			t.Errorf("interface endpoint %q should have privateDnsEnabled=true", ep.Name)
		}
	}
}

// ---------------------------------------------------------------------------
// Test: IAM roles have correct policies attached
// ---------------------------------------------------------------------------

func TestIAMRolesCreated(t *testing.T) {
	mocks := &networkMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		_, err := network.NewIAMRoles(ctx, &network.IAMArgs{
			Environment:     "dev",
			DataBucketArn:   pulumi.String("arn:aws:s3:::kaizen-dev-data").ToStringOutput(),
			MlflowBucketArn: pulumi.String("arn:aws:s3:::kaizen-dev-mlflow").ToStringOutput(),
		})
		return err
	}, pulumi.WithMocks("kaizen", "dev", mocks), withConfig(defaultConfig()))
	if err != nil {
		t.Fatalf("pulumi.RunErr: %v", err)
	}

	roles := mocks.byType(typeIAMRole)

	expectedRoles := map[string]bool{
		"ecs-task-role":  false,
		"ecs-exec-role":  false,
		"ci-deploy-role": false,
	}

	for _, role := range roles {
		if _, ok := expectedRoles[role.Name]; ok {
			expectedRoles[role.Name] = true
		}
	}

	for name, found := range expectedRoles {
		if !found {
			t.Errorf("IAM role %q not created", name)
		}
	}
}

func TestEcsTaskRoleTrustPolicy(t *testing.T) {
	mocks := &networkMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		_, err := network.NewIAMRoles(ctx, &network.IAMArgs{
			Environment:     "dev",
			DataBucketArn:   pulumi.String("arn:aws:s3:::kaizen-dev-data").ToStringOutput(),
			MlflowBucketArn: pulumi.String("arn:aws:s3:::kaizen-dev-mlflow").ToStringOutput(),
		})
		return err
	}, pulumi.WithMocks("kaizen", "dev", mocks), withConfig(defaultConfig()))
	if err != nil {
		t.Fatalf("pulumi.RunErr: %v", err)
	}

	for _, role := range mocks.byType(typeIAMRole) {
		if role.Name != "ecs-task-role" && role.Name != "ecs-exec-role" {
			continue
		}

		trustDoc := role.Inputs["assumeRolePolicy"].StringValue()
		var doc map[string]interface{}
		if err := json.Unmarshal([]byte(trustDoc), &doc); err != nil {
			t.Fatalf("role %q: failed to parse trust policy: %v", role.Name, err)
		}

		stmts, ok := doc["Statement"].([]interface{})
		if !ok || len(stmts) == 0 {
			t.Errorf("role %q: trust policy has no statements", role.Name)
			continue
		}

		stmt := stmts[0].(map[string]interface{})
		principal := stmt["Principal"].(map[string]interface{})
		service, ok := principal["Service"].(string)
		if !ok || service != "ecs-tasks.amazonaws.com" {
			t.Errorf("role %q: trust policy principal = %v, want ecs-tasks.amazonaws.com", role.Name, principal)
		}
	}
}

func TestTaskRoleHasXRayPolicy(t *testing.T) {
	mocks := &networkMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		_, err := network.NewIAMRoles(ctx, &network.IAMArgs{
			Environment:     "dev",
			DataBucketArn:   pulumi.String("arn:aws:s3:::kaizen-dev-data").ToStringOutput(),
			MlflowBucketArn: pulumi.String("arn:aws:s3:::kaizen-dev-mlflow").ToStringOutput(),
		})
		return err
	}, pulumi.WithMocks("kaizen", "dev", mocks), withConfig(defaultConfig()))
	if err != nil {
		t.Fatalf("pulumi.RunErr: %v", err)
	}

	found := false
	for _, att := range mocks.byType(typeIAMAttachment) {
		if att.Name != "task-xray-policy" {
			continue
		}
		found = true

		arn := att.Inputs["policyArn"].StringValue()
		if !strings.Contains(arn, "AWSXRayDaemonWriteAccess") {
			t.Errorf("X-Ray policy ARN = %q, want AWSXRayDaemonWriteAccess", arn)
		}
	}

	if !found {
		t.Error("X-Ray policy attachment for task role not found")
	}
}

func TestExecRoleHasECSExecutionPolicy(t *testing.T) {
	mocks := &networkMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		_, err := network.NewIAMRoles(ctx, &network.IAMArgs{
			Environment:     "dev",
			DataBucketArn:   pulumi.String("arn:aws:s3:::kaizen-dev-data").ToStringOutput(),
			MlflowBucketArn: pulumi.String("arn:aws:s3:::kaizen-dev-mlflow").ToStringOutput(),
		})
		return err
	}, pulumi.WithMocks("kaizen", "dev", mocks), withConfig(defaultConfig()))
	if err != nil {
		t.Fatalf("pulumi.RunErr: %v", err)
	}

	found := false
	for _, att := range mocks.byType(typeIAMAttachment) {
		if att.Name != "exec-ecs-policy" {
			continue
		}
		found = true

		arn := att.Inputs["policyArn"].StringValue()
		if !strings.Contains(arn, "AmazonECSTaskExecutionRolePolicy") {
			t.Errorf("ECS exec policy ARN = %q, want AmazonECSTaskExecutionRolePolicy", arn)
		}
	}

	if !found {
		t.Error("ECS execution role policy attachment not found")
	}
}

func TestTaskRoleSecretsPolicy(t *testing.T) {
	mocks := &networkMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		_, err := network.NewIAMRoles(ctx, &network.IAMArgs{
			Environment:     "dev",
			DataBucketArn:   pulumi.String("arn:aws:s3:::kaizen-dev-data").ToStringOutput(),
			MlflowBucketArn: pulumi.String("arn:aws:s3:::kaizen-dev-mlflow").ToStringOutput(),
		})
		return err
	}, pulumi.WithMocks("kaizen", "dev", mocks), withConfig(defaultConfig()))
	if err != nil {
		t.Fatalf("pulumi.RunErr: %v", err)
	}

	for _, policy := range mocks.byType(typeIAMRolePolicy) {
		if policy.Name != "task-secrets-policy" {
			continue
		}

		policyJSON := policy.Inputs["policy"].StringValue()
		var doc map[string]interface{}
		if err := json.Unmarshal([]byte(policyJSON), &doc); err != nil {
			t.Fatalf("failed to parse secrets policy: %v", err)
		}

		stmts := doc["Statement"].([]interface{})
		stmt := stmts[0].(map[string]interface{})

		// Verify action is GetSecretValue only
		actions := stmt["Action"].([]interface{})
		if len(actions) != 1 || actions[0].(string) != "secretsmanager:GetSecretValue" {
			t.Errorf("secrets policy actions = %v, want [secretsmanager:GetSecretValue]", actions)
		}

		// Verify resource is scoped to kaizen/dev/*
		resources := stmt["Resource"].([]interface{})
		if len(resources) != 1 {
			t.Fatalf("expected 1 resource ARN, got %d", len(resources))
		}
		arn := resources[0].(string)
		if !strings.Contains(arn, "kaizen/dev/*") {
			t.Errorf("secrets policy resource = %q, want scoped to kaizen/dev/*", arn)
		}
		return
	}

	t.Error("task-secrets-policy not found")
}

func TestCIDeployRoleHasOIDCTrust(t *testing.T) {
	mocks := &networkMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		_, err := network.NewIAMRoles(ctx, &network.IAMArgs{
			Environment:     "dev",
			DataBucketArn:   pulumi.String("arn:aws:s3:::kaizen-dev-data").ToStringOutput(),
			MlflowBucketArn: pulumi.String("arn:aws:s3:::kaizen-dev-mlflow").ToStringOutput(),
		})
		return err
	}, pulumi.WithMocks("kaizen", "dev", mocks), withConfig(defaultConfig()))
	if err != nil {
		t.Fatalf("pulumi.RunErr: %v", err)
	}

	// Verify the OIDC provider is created for GitHub Actions.
	oidcProviders := mocks.byType(typeOIDCProvider)
	if len(oidcProviders) == 0 {
		t.Fatal("GitHub OIDC provider not created")
	}

	url := oidcProviders[0].Inputs["url"].StringValue()
	if url != "https://token.actions.githubusercontent.com" {
		t.Errorf("OIDC provider URL = %q, want GitHub Actions token URL", url)
	}
}

func TestCIDeployRoleECSPolicy(t *testing.T) {
	mocks := &networkMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		_, err := network.NewIAMRoles(ctx, &network.IAMArgs{
			Environment:     "dev",
			DataBucketArn:   pulumi.String("arn:aws:s3:::kaizen-dev-data").ToStringOutput(),
			MlflowBucketArn: pulumi.String("arn:aws:s3:::kaizen-dev-mlflow").ToStringOutput(),
		})
		return err
	}, pulumi.WithMocks("kaizen", "dev", mocks), withConfig(defaultConfig()))
	if err != nil {
		t.Fatalf("pulumi.RunErr: %v", err)
	}

	for _, policy := range mocks.byType(typeIAMRolePolicy) {
		if policy.Name != "ci-ecs-policy" {
			continue
		}

		policyJSON := policy.Inputs["policy"].StringValue()
		var doc map[string]interface{}
		if err := json.Unmarshal([]byte(policyJSON), &doc); err != nil {
			t.Fatalf("failed to parse CI ECS policy: %v", err)
		}

		stmts := doc["Statement"].([]interface{})
		stmt := stmts[0].(map[string]interface{})
		actions := stmt["Action"].([]interface{})

		requiredActions := map[string]bool{
			"ecs:UpdateService":            false,
			"ecs:RegisterTaskDefinition":   false,
			"ecs:DeregisterTaskDefinition": false,
		}
		for _, a := range actions {
			if _, ok := requiredActions[a.(string)]; ok {
				requiredActions[a.(string)] = true
			}
		}
		for action, found := range requiredActions {
			if !found {
				t.Errorf("CI ECS policy missing action %q", action)
			}
		}
		return
	}

	t.Error("ci-ecs-policy not found")
}

func TestCIDeployRoleECRPolicyIsScoped(t *testing.T) {
	mocks := &networkMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		_, err := network.NewIAMRoles(ctx, &network.IAMArgs{
			Environment:     "dev",
			DataBucketArn:   pulumi.String("arn:aws:s3:::kaizen-dev-data").ToStringOutput(),
			MlflowBucketArn: pulumi.String("arn:aws:s3:::kaizen-dev-mlflow").ToStringOutput(),
		})
		return err
	}, pulumi.WithMocks("kaizen", "dev", mocks), withConfig(defaultConfig()))
	if err != nil {
		t.Fatalf("pulumi.RunErr: %v", err)
	}

	for _, policy := range mocks.byType(typeIAMRolePolicy) {
		if policy.Name != "ci-ecr-policy" {
			continue
		}

		policyJSON := policy.Inputs["policy"].StringValue()
		var doc map[string]interface{}
		if err := json.Unmarshal([]byte(policyJSON), &doc); err != nil {
			t.Fatalf("failed to parse CI ECR policy: %v", err)
		}

		stmts := doc["Statement"].([]interface{})

		// At least one statement should scope ECR push to kaizen-* repos.
		scopedFound := false
		for _, s := range stmts {
			stmt := s.(map[string]interface{})
			resources := stmt["Resource"].([]interface{})
			for _, r := range resources {
				if strings.Contains(r.(string), "repository/kaizen-*") {
					scopedFound = true
				}
			}
		}

		if !scopedFound {
			t.Error("CI ECR policy is not scoped to kaizen-* repositories")
		}
		return
	}

	t.Error("ci-ecr-policy not found")
}

// ---------------------------------------------------------------------------
// Test: Full network stack integration (end-to-end wiring)
// ---------------------------------------------------------------------------

func TestFullNetworkStackIntegration(t *testing.T) {
	mocks := &networkMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		// 1. VPC
		vpcOut, err := network.NewVpc(ctx)
		if err != nil {
			return err
		}

		// 2. Security groups
		sgResult, err := network.NewSecurityGroups(ctx, "kaizen", &network.SecurityGroupsArgs{
			VpcId: vpcOut.VpcId,
		})
		if err != nil {
			return err
		}

		// 3. Cloud Map namespace
		_, err = network.NewServiceDiscovery(ctx, &network.ServiceDiscoveryArgs{
			VpcId: vpcOut.VpcId,
		})
		if err != nil {
			return err
		}

		// 4. VPC endpoints
		_, err = network.NewVpcEndpoints(ctx, &network.VpcEndpointArgs{
			VpcId:                vpcOut.VpcId,
			PrivateSubnetIds:     vpcOut.PrivateSubnetIds,
			PrivateRouteTableIds: vpcOut.PrivateRouteTableIds,
			EcsSecurityGroupId:   sgResult.Groups["ecs"],
			M4bSecurityGroupId:   sgResult.Groups["m4b"],
		})
		if err != nil {
			return err
		}

		// 5. IAM roles
		_, err = network.NewIAMRoles(ctx, &network.IAMArgs{
			Environment:     "dev",
			DataBucketArn:   pulumi.String("arn:aws:s3:::kaizen-dev-data").ToStringOutput(),
			MlflowBucketArn: pulumi.String("arn:aws:s3:::kaizen-dev-mlflow").ToStringOutput(),
		})
		return err
	}, pulumi.WithMocks("kaizen", "dev", mocks), withConfig(defaultConfig()))
	if err != nil {
		t.Fatalf("full network stack failed: %v", err)
	}

	// Verify key resource counts from the full stack.
	t.Run("vpc_count", func(t *testing.T) {
		if n := len(mocks.byType(typeVpc)); n != 1 {
			t.Errorf("VPC count = %d, want 1", n)
		}
	})

	t.Run("subnet_count", func(t *testing.T) {
		if n := len(mocks.byType(typeSubnet)); n != 6 {
			t.Errorf("subnet count = %d, want 6 (3 public + 3 private)", n)
		}
	})

	t.Run("security_group_count", func(t *testing.T) {
		// 6 from NewSecurityGroups + 1 VPCE SG from NewVpcEndpoints = 7
		sgs := mocks.byType(typeSecurityGroup)
		if len(sgs) < 7 {
			t.Errorf("security group count = %d, want >= 7", len(sgs))
		}
	})

	t.Run("vpc_endpoint_count", func(t *testing.T) {
		// 5 endpoints: S3, ECR DKR, ECR API, CW Logs, Secrets Manager
		eps := mocks.byType(typeVpcEndpoint)
		if len(eps) != 5 {
			t.Errorf("VPC endpoint count = %d, want 5", len(eps))
		}
	})

	t.Run("cloud_map_namespace", func(t *testing.T) {
		ns := mocks.byType(typePrivateDnsNS)
		if len(ns) != 1 {
			t.Errorf("Cloud Map namespace count = %d, want 1", len(ns))
		}
	})

	t.Run("iam_role_count", func(t *testing.T) {
		roles := mocks.byType(typeIAMRole)
		if len(roles) != 3 {
			t.Errorf("IAM role count = %d, want 3", len(roles))
		}
	})

	t.Run("no_open_cidr_on_internal", func(t *testing.T) {
		internalPrefixes := []string{"-ecs-", "-rds-", "-msk-", "-redis-", "-m4b-"}
		for _, rule := range mocks.byType(typeSGRule) {
			cidrs, ok := rule.Inputs["cidrBlocks"]
			if !ok || !cidrs.IsArray() {
				continue
			}
			for _, cidr := range cidrs.ArrayValue() {
				if cidr.StringValue() != "0.0.0.0/0" {
					continue
				}
				for _, prefix := range internalPrefixes {
					if strings.Contains(rule.Name, prefix) {
						t.Errorf("rule %q has 0.0.0.0/0 on internal group", rule.Name)
					}
				}
			}
		}
	})
}
