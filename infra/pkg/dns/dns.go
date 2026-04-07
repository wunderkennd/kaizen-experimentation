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

// Args holds all inputs required by the DNS module.
type Args struct {
	Config config.KaizenConfig
	ALB    config.ALBOutputs
}

// Outputs holds all resources exported by the DNS module.
type Outputs struct {
	HostedZoneID   pulumi.IDOutput
	CertificateArn pulumi.StringOutput
}

// NewDNS creates the Route 53 hosted zone, ACM wildcard certificate, DNS
// validation records, and A-record aliases for the Kaizen platform.
//
// Zone: kaizen.{domain}
// Cert: *.kaizen.{domain} (DNS-validated via Route 53)
// A records: root → ALB, assign.kaizen.{domain} → ALB, api.kaizen.{domain} → ALB
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

	// --- A Records (alias to ALB) ---
	// Each record is an alias record pointing to the ALB's DNS name and hosted zone.
	aliasRecords := []struct {
		name     string
		dnsName  string // subdomain prefix (empty string = zone apex)
	}{
		{name: "root", dnsName: zoneName},
		{name: "assign", dnsName: fmt.Sprintf("assign.%s", zoneName)},
		{name: "api", dnsName: fmt.Sprintf("api.%s", zoneName)},
	}

	for _, rec := range aliasRecords {
		_, err := route53.NewRecord(ctx, fmt.Sprintf("kaizen-alias-%s", rec.name), &route53.RecordArgs{
			ZoneId: zone.ZoneId,
			Name:   pulumi.String(rec.dnsName),
			Type:   pulumi.String("A"),
			Aliases: route53.RecordAliasArray{
				&route53.RecordAliasArgs{
					Name:                 args.ALB.ALBDNSName,
					ZoneId:               args.ALB.ALBHostedZoneID,
					EvaluateTargetHealth: pulumi.Bool(true),
				},
			},
		})
		if err != nil {
			return nil, fmt.Errorf("creating alias record %q: %w", rec.name, err)
		}
	}

	return &Outputs{
		HostedZoneID:   zone.ID(),
		CertificateArn: certValidation.CertificateArn,
	}, nil
}
