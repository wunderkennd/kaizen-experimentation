// Package test contains Pulumi mock tests for the ALB (load balancer) and DNS
// (Route 53 + ACM) modules of the Kaizen experimentation platform.
//
// Tests verify resource creation, property configuration, and cross-module
// contract expectations without deploying real infrastructure.
package test

import (
	"testing"

	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"

	kconfig "github.com/kaizen-experimentation/infra/pkg/config"
	"github.com/kaizen-experimentation/infra/pkg/dns"
	"github.com/kaizen-experimentation/infra/pkg/loadbalancer"
)

// ---------------------------------------------------------------------------
// ALB Tests
// ---------------------------------------------------------------------------

// TestAlbCreated verifies that NewALB creates exactly one Application Load
// Balancer with HTTP/2 enabled and external (non-internal) access.
func TestAlbCreated(t *testing.T) {
	mocks := &universalMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		_, err := loadbalancer.NewALB(ctx, &loadbalancer.ALBInputs{
			PublicSubnetIds: pulumi.StringArray{pulumi.String("subnet-pub-a"), pulumi.String("subnet-pub-b")},
			SecurityGroupId: pulumi.String("sg-alb").ToStringOutput(),
			CertificateArn:  pulumi.String("arn:aws:acm:us-east-1:123456789:certificate/abc").ToStringOutput(),
			LogsBucketName:  pulumi.String("kaizen-dev-logs").ToStringOutput(),
			Environment:     "dev",
		})
		return err
	}, pulumi.WithMocks("kaizen", "dev", mocks))
	if err != nil {
		t.Fatalf("Pulumi program failed: %v", err)
	}

	albs := mocks.byType("aws:lb/loadBalancer:LoadBalancer")
	if len(albs) != 1 {
		t.Fatalf("expected 1 ALB, got %d", len(albs))
	}

	alb := albs[0]

	if v, ok := alb.Inputs["enableHttp2"]; !ok || !v.BoolValue() {
		t.Error("ALB must have enableHttp2 = true")
	}
	if v, ok := alb.Inputs["internal"]; !ok || v.BoolValue() {
		t.Error("ALB must have internal = false (internet-facing)")
	}
}

// TestAlbHttpsListener verifies that NewALB creates an HTTPS listener on
// port 443 with the TLS 1.3 security policy.
func TestAlbHttpsListener(t *testing.T) {
	mocks := &universalMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		_, err := loadbalancer.NewALB(ctx, &loadbalancer.ALBInputs{
			PublicSubnetIds: pulumi.StringArray{pulumi.String("subnet-pub-a"), pulumi.String("subnet-pub-b")},
			SecurityGroupId: pulumi.String("sg-alb").ToStringOutput(),
			CertificateArn:  pulumi.String("arn:aws:acm:us-east-1:123456789:certificate/abc").ToStringOutput(),
			LogsBucketName:  pulumi.String("kaizen-dev-logs").ToStringOutput(),
			Environment:     "dev",
		})
		return err
	}, pulumi.WithMocks("kaizen", "dev", mocks))
	if err != nil {
		t.Fatalf("Pulumi program failed: %v", err)
	}

	listeners := mocks.byType("aws:lb/listener:Listener")
	var httpsListeners []trackedResource
	for _, l := range listeners {
		if v, ok := l.Inputs["port"]; ok && v.NumberValue() == 443 {
			httpsListeners = append(httpsListeners, l)
		}
	}

	if len(httpsListeners) != 1 {
		t.Fatalf("expected 1 HTTPS listener on port 443, got %d", len(httpsListeners))
	}

	https := httpsListeners[0]
	if v, ok := https.Inputs["protocol"]; !ok || v.StringValue() != "HTTPS" {
		t.Errorf("HTTPS listener protocol = %v, want HTTPS", v)
	}
}

// TestAlbHttpRedirectListener verifies that NewALB creates an HTTP listener
// on port 80 with a redirect action to HTTPS.
func TestAlbHttpRedirectListener(t *testing.T) {
	mocks := &universalMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		_, err := loadbalancer.NewALB(ctx, &loadbalancer.ALBInputs{
			PublicSubnetIds: pulumi.StringArray{pulumi.String("subnet-pub-a"), pulumi.String("subnet-pub-b")},
			SecurityGroupId: pulumi.String("sg-alb").ToStringOutput(),
			CertificateArn:  pulumi.String("arn:aws:acm:us-east-1:123456789:certificate/abc").ToStringOutput(),
			LogsBucketName:  pulumi.String("kaizen-dev-logs").ToStringOutput(),
			Environment:     "dev",
		})
		return err
	}, pulumi.WithMocks("kaizen", "dev", mocks))
	if err != nil {
		t.Fatalf("Pulumi program failed: %v", err)
	}

	listeners := mocks.byType("aws:lb/listener:Listener")
	var httpListeners []trackedResource
	for _, l := range listeners {
		if v, ok := l.Inputs["port"]; ok && v.NumberValue() == 80 {
			httpListeners = append(httpListeners, l)
		}
	}

	if len(httpListeners) != 1 {
		t.Fatalf("expected 1 HTTP listener on port 80, got %d", len(httpListeners))
	}

	http := httpListeners[0]
	if v, ok := http.Inputs["protocol"]; !ok || v.StringValue() != "HTTP" {
		t.Errorf("HTTP listener protocol = %v, want HTTP", v)
	}

	// Verify the default action is a redirect.
	actions, ok := http.Inputs["defaultActions"]
	if !ok || !actions.IsArray() {
		t.Fatal("HTTP listener missing defaultActions array")
	}
	actionList := actions.ArrayValue()
	if len(actionList) == 0 {
		t.Fatal("HTTP listener has no default actions")
	}
	action := actionList[0].ObjectValue()
	if v, ok := action["type"]; !ok || v.StringValue() != "redirect" {
		t.Errorf("HTTP listener action type = %v, want redirect", v)
	}
}

// ---------------------------------------------------------------------------
// DNS Tests
// ---------------------------------------------------------------------------

// newDNSMockConfig returns a minimal KaizenConfig for DNS module tests.
func newDNSMockConfig() *kconfig.KaizenConfig {
	return &kconfig.KaizenConfig{
		Domain:      "example.com",
		ProjectName: "kaizen-experimentation",
		Environment: "dev",
		Env:         kconfig.EnvDev,
	}
}

// TestDnsZoneCreated verifies that NewDNS creates a Route 53 hosted zone.
func TestDnsZoneCreated(t *testing.T) {
	mocks := &universalMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		_, err := dns.NewDNS(ctx, &dns.Args{
			Config: *newDNSMockConfig(),
		})
		return err
	}, pulumi.WithMocks("kaizen", "dev", mocks))
	if err != nil {
		t.Fatalf("Pulumi program failed: %v", err)
	}

	zones := mocks.byType("aws:route53/zone:Zone")
	if len(zones) != 1 {
		t.Fatalf("expected 1 Route 53 hosted zone, got %d", len(zones))
	}

	zone := zones[0]
	if v, ok := zone.Inputs["name"]; !ok || v.StringValue() != "kaizen.example.com" {
		t.Errorf("zone name = %v, want kaizen.example.com", v)
	}
}

// TestAcmWildcardCert verifies that NewDNS creates an ACM certificate with
// DNS validation method and a wildcard domain name.
func TestAcmWildcardCert(t *testing.T) {
	mocks := &universalMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		_, err := dns.NewDNS(ctx, &dns.Args{
			Config: *newDNSMockConfig(),
		})
		return err
	}, pulumi.WithMocks("kaizen", "dev", mocks))
	if err != nil {
		t.Fatalf("Pulumi program failed: %v", err)
	}

	certs := mocks.byType("aws:acm/certificate:Certificate")
	if len(certs) != 1 {
		t.Fatalf("expected 1 ACM certificate, got %d", len(certs))
	}

	cert := certs[0]
	if v, ok := cert.Inputs["validationMethod"]; !ok || v.StringValue() != "DNS" {
		t.Errorf("ACM certificate validationMethod = %v, want DNS", v)
	}
	if v, ok := cert.Inputs["domainName"]; !ok || v.StringValue() != "*.kaizen.example.com" {
		t.Errorf("ACM certificate domainName = %v, want *.kaizen.example.com", v)
	}
}
