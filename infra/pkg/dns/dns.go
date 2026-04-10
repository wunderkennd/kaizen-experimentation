// Package dns provisions Route 53 hosted zones, ACM wildcard certificates with
// DNS validation, and A-record aliases pointing to the ALB.
//
// Owner: Infra-5 (task I.0.14)
//
// Outputs consumed by:
//   - pkg/loadbalancer (CertificateArn → HTTPS listener)
//   - main.go (Pulumi stack exports)
package dns

import (
	"fmt"

	"github.com/pulumi/pulumi-aws/sdk/v6/go/aws/acm"
	"github.com/pulumi/pulumi-aws/sdk/v6/go/aws/route53"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"

	"github.com/kaizen-experimentation/infra/pkg/config"
)

// Args holds all inputs required by the DNS zone and certificate module.
type Args struct {
	Config config.KaizenConfig
}

// AliasArgs holds inputs for creating Route 53 alias records pointing to the ALB.
// This is separated from NewDNS to break the circular dependency:
// DNS cert → ALB → DNS alias records.
type AliasArgs struct {
	// ZoneId is the Route 53 hosted zone ID (from NewDNS).
	ZoneId pulumi.IDOutput
	// ZoneName is the fully-qualified zone name (e.g., "kaizen.example.com").
	ZoneName string
	// ALBDnsName is the ALB's DNS name for alias targets.
	ALBDnsName pulumi.StringOutput
	// ALBZoneId is the ALB's canonical hosted zone ID for alias targets.
	ALBZoneId pulumi.StringOutput
}

// Outputs holds all resources exported by the DNS module.
type Outputs struct {
	HostedZoneID   pulumi.IDOutput
	CertificateArn pulumi.StringOutput
}

// NewDNS creates the Route 53 hosted zone and ACM wildcard certificate with
// DNS validation. A-record aliases are created separately via NewDNSAliases
// to break the circular dependency with the ALB (which needs the certificate).
//
// Zone: kaizen.{domain}
// Cert: *.kaizen.{domain} (DNS-validated via Route 53)
func NewDNS(ctx *pulumi.Context, args *Args) (*Outputs, error) {
	zoneName := fmt.Sprintf("kaizen.%s", args.Config.Domain)

	// --- Route 53 Hosted Zone ---
	zone, err := route53.NewZone(ctx, "kaizen-zone", &route53.ZoneArgs{
		Name: pulumi.String(zoneName),
		Tags: pulumi.StringMap{
			"Project":     pulumi.String(args.Config.ProjectName),
			"Environment": pulumi.String(args.Config.Environment),
			"ManagedBy":   pulumi.String("pulumi"),
		},
	})
	if err != nil {
		return nil, fmt.Errorf("creating Route 53 zone: %w", err)
	}

	// --- ACM Wildcard Certificate ---
	wildcardDomain := fmt.Sprintf("*.%s", zoneName)

	cert, err := acm.NewCertificate(ctx, "kaizen-wildcard-cert", &acm.CertificateArgs{
		DomainName:       pulumi.String(wildcardDomain),
		ValidationMethod: pulumi.String("DNS"),
		SubjectAlternativeNames: pulumi.StringArray{
			pulumi.String(zoneName), // include bare domain as SAN
		},
		Tags: pulumi.StringMap{
			"Project":     pulumi.String(args.Config.ProjectName),
			"Environment": pulumi.String(args.Config.Environment),
			"ManagedBy":   pulumi.String("pulumi"),
		},
	})
	if err != nil {
		return nil, fmt.Errorf("creating ACM certificate: %w", err)
	}

	// --- DNS Validation Record ---
	// ACM returns domain_validation_options; we create the CNAME record in our
	// hosted zone so ACM can verify domain ownership.
	validationRecord, err := route53.NewRecord(ctx, "kaizen-cert-validation", &route53.RecordArgs{
		ZoneId: zone.ZoneId,
		Name: cert.DomainValidationOptions.ApplyT(func(opts []acm.CertificateDomainValidationOption) string {
			return *opts[0].ResourceRecordName
		}).(pulumi.StringOutput),
		Type: cert.DomainValidationOptions.ApplyT(func(opts []acm.CertificateDomainValidationOption) string {
			return *opts[0].ResourceRecordType
		}).(pulumi.StringOutput),
		Records: pulumi.StringArray{
			cert.DomainValidationOptions.ApplyT(func(opts []acm.CertificateDomainValidationOption) string {
				return *opts[0].ResourceRecordValue
			}).(pulumi.StringOutput),
		},
		Ttl:            pulumi.Int(300),
		AllowOverwrite: pulumi.Bool(true),
	})
	if err != nil {
		return nil, fmt.Errorf("creating certificate validation record: %w", err)
	}

	// --- Wait for Certificate Validation ---
	certValidation, err := acm.NewCertificateValidation(ctx, "kaizen-cert-validation-wait", &acm.CertificateValidationArgs{
		CertificateArn: cert.Arn,
		ValidationRecordFqdns: pulumi.StringArray{
			validationRecord.Fqdn,
		},
	})
	if err != nil {
		return nil, fmt.Errorf("creating certificate validation waiter: %w", err)
	}

	return &Outputs{
		HostedZoneID:   zone.ID(),
		CertificateArn: certValidation.CertificateArn,
	}, nil
}

// NewDNSAliases creates Route 53 A-record aliases pointing to the ALB.
// Called after the ALB is created to resolve the DNS↔ALB circular dependency.
//
// Records: root → ALB, assign.{zone} → ALB, api.{zone} → ALB
func NewDNSAliases(ctx *pulumi.Context, args *AliasArgs) error {
	aliasRecords := []struct {
		name    string
		dnsName string
	}{
		{name: "root", dnsName: args.ZoneName},
		{name: "assign", dnsName: fmt.Sprintf("assign.%s", args.ZoneName)},
		{name: "api", dnsName: fmt.Sprintf("api.%s", args.ZoneName)},
	}

	for _, rec := range aliasRecords {
		_, err := route53.NewRecord(ctx, fmt.Sprintf("kaizen-alias-%s", rec.name), &route53.RecordArgs{
			ZoneId: args.ZoneId.ToStringOutput(),
			Name:   pulumi.String(rec.dnsName),
			Type:   pulumi.String("A"),
			Aliases: route53.RecordAliasArray{
				&route53.RecordAliasArgs{
					Name:                 args.ALBDnsName,
					ZoneId:               args.ALBZoneId,
					EvaluateTargetHealth: pulumi.Bool(true),
				},
			},
		})
		if err != nil {
			return fmt.Errorf("creating alias record %q: %w", rec.name, err)
		}
	}

	return nil
}
