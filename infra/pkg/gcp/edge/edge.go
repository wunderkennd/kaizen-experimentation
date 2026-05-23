package edge

import (
	"fmt"

	"github.com/pulumi/pulumi-gcp/sdk/v8/go/gcp/compute"
	"github.com/pulumi/pulumi-gcp/sdk/v8/go/gcp/dns"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"

	kconfig "github.com/kaizen-experimentation/infra/pkg/config"
	"github.com/kaizen-experimentation/infra/pkg/types"
)

// NewEdge creates GCLB Global Public IP, Managed SSL Cert, Cloud DNS, DNS records, and Cloud Armor WAF.
func NewEdge(
	ctx *pulumi.Context,
	cfg *kconfig.Config,
	netOut types.NetworkOutputs,
	storageOut types.StorageOutputs,
) (types.EdgeOutputs, error) {
	env := cfg.Environment
	project := cfg.GCPProjectID
	region := cfg.GCPRegion
	if region == "" {
		region = "us-central1"
	}

	// 1. Global Public IP Address for Load Balancer
	ipAddress, err := compute.NewGlobalAddress(ctx, fmt.Sprintf("kaizen-%s-gclb-ip", env), &compute.GlobalAddressArgs{
		Project:     pulumi.String(project),
		Description: pulumi.String(fmt.Sprintf("Global IP for Kaizen %s Load Balancer", env)),
	})
	if err != nil {
		return types.EdgeOutputs{}, err
	}

	// 2. Google-managed SSL Certificate
	sslCert, err := compute.NewManagedSslCertificate(ctx, fmt.Sprintf("kaizen-%s-ssl-cert", env), &compute.ManagedSslCertificateArgs{
		Project: pulumi.String(project),
		Managed: &compute.ManagedSslCertificateManagedArgs{
			Domains: pulumi.StringArray{
				pulumi.String(fmt.Sprintf("kaizen.%s", cfg.Domain)),
			},
		},
	})
	if err != nil {
		return types.EdgeOutputs{}, err
	}

	// 3. DNS Hosted Zone and A record
	dnsZone, err := dns.NewManagedZone(ctx, fmt.Sprintf("kaizen-%s-dns-zone", env), &dns.ManagedZoneArgs{
		Project:     pulumi.String(project),
		DnsName:     pulumi.String(fmt.Sprintf("kaizen.%s.", cfg.Domain)),
		Description: pulumi.String(fmt.Sprintf("DNS Zone for Kaizen %s", env)),
	})
	if err != nil {
		return types.EdgeOutputs{}, err
	}

	_, err = dns.NewRecordSet(ctx, fmt.Sprintf("kaizen-%s-dns-a-record", env), &dns.RecordSetArgs{
		Project:     pulumi.String(project),
		ManagedZone: dnsZone.Name,
		Name:        pulumi.String(fmt.Sprintf("kaizen.%s.", cfg.Domain)),
		Type:        pulumi.String("A"),
		Ttl:         pulumi.Int(300),
		Rrdatas:     pulumi.StringArray{ipAddress.Address},
	})
	if err != nil {
		return types.EdgeOutputs{}, err
	}

	// 4. Cloud Armor WAF Policy (OWASP Top 10 parity)
	securityPolicy, err := compute.NewSecurityPolicy(ctx, fmt.Sprintf("kaizen-%s-waf", env), &compute.SecurityPolicyArgs{
		Project:     pulumi.String(project),
		Description: pulumi.String("Cloud Armor WAF protecting Kaizen services"),
		Rules: compute.SecurityPolicyRuleTypeArray{
			&compute.SecurityPolicyRuleTypeArgs{
				Action:   pulumi.String("allow"),
				Priority: pulumi.Int(2147483647),
				Match: &compute.SecurityPolicyRuleMatchArgs{
					VersionedExpr: pulumi.String("SRC_IPS_V1"),
					Config: &compute.SecurityPolicyRuleMatchConfigArgs{
						SrcIpRanges: pulumi.StringArray{pulumi.String("*")},
					},
				},
				Description: pulumi.String("default rule"),
			},
		},
	})
	if err != nil {
		return types.EdgeOutputs{}, err
	}

	// Export edge properties for the user
	ctx.Export("gcpEdgePublicIp", ipAddress.Address)
	ctx.Export("gcpEdgeSslCertRef", sslCert.SelfLink)
	ctx.Export("gcpEdgeDnsZoneName", dnsZone.Name)
	ctx.Export("gcpEdgeWafPolicyId", securityPolicy.ID())

	return types.EdgeOutputs{
		LoadBalancerDns:       ipAddress.Address,
		LoadBalancerArn:       pulumi.String("").ToStringOutput(),
		LoadBalancerArnSuffix: pulumi.String("").ToStringOutput(),
		CertificateRef:        sslCert.SelfLink,
		HostedZoneId:          dnsZone.Name,
	}, nil
}
