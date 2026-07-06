// Package gcp — edge.go provisions the GCP public edge (Stage 6), the parity
// arm of pkg/aws.NewEdge: a global external HTTPS load balancer
// (EXTERNAL_MANAGED) with one serverless NEG + backend service per Cloud Run
// service, Cloud DNS zone + A records, a Google-managed certificate, and a
// Cloud Armor security policy at AWS WAF v2 parity. Issue #496 (multi-cloud
// spec Phase 3).
//
// # Routing (URL map)
//
// The four AWS ALB rules are carried verbatim (see
// pkg/aws/loadbalancer/target_groups.go):
//
//	assign.kaizen.{domain}  →  m1  (host rule; ALB priority 10)
//	/api/*                  →  m5  (ALB priority 20)
//	/flags/*                →  m7  (ALB priority 30)
//	/* (default)            →  m6  (ALB priority 100)
//
// Issue #496 additionally requires the LB to route ALL 8 Cloud Run services
// (on AWS the remaining four are reachable only via Cloud Map service
// discovery). These prefixes are therefore a deliberate GCP-only superset,
// recorded here for the parity audit (#503):
//
//	/ingest/*               →  m2-pipeline
//	/orchestration/*        →  m2-orch
//	/metrics/*              →  m3
//	/analysis/*             →  m4a
//
// Every fronted service was created with ingress
// INGRESS_TRAFFIC_INTERNAL_LOAD_BALANCER (pkg/gcp/compute), so the allUsers
// run.invoker binding this module adds still only admits traffic that came
// through the load balancer — the GCP analogue of ECS tasks on private
// subnets behind the ALB.
//
// # Certificate
//
// Google-managed certificates do not support wildcard domains, so ACM's
// *.kaizen.{domain} becomes the enumerated SAN set kaizen.{domain},
// assign.kaizen.{domain}, api.kaizen.{domain} — exactly the three A records
// pkg/aws/dns.NewDNSAliases creates.
//
// # Cloud Armor ↔ AWS WAF v2 parity (manual diff, issue #496)
//
//	AWS WAF v2 (pkg/aws/waf)                Cloud Armor (this module)
//	─────────────────────────────────────   ─────────────────────────────────────
//	rate-limit-per-ip (prio 1, Block,       prio 1: throttle, enforceOnKey=IP,
//	  RateBasedStatement limit/5min/IP)       exceed=deny(429), count/300s
//	geo-block (prio 2, optional)            prio 2: deny(403) on
//	                                          origin.region_code expression
//	AWSManagedRulesCommonRuleSet (prio 10)  prios 10-16: deny(403) on
//	                                          evaluatePreconfiguredWaf of
//	                                          xss/lfi/rfi/rce/scannerdetection/
//	                                          protocolattack/sessionfixation
//	                                          -v33-stable, sensitivity 1
//	AWSManagedRulesSQLiRuleSet (prio 20)    prio 20: deny(403) on
//	                                          sqli-v33-stable, sensitivity 1
//	DefaultAction Allow                     prio 2147483647: allow *
//	WebAclAssociation (ALB)                 BackendService.SecurityPolicy on
//	                                          every backend
//	S3 logging (aws-waf-logs-*)             backend LogConfig → Cloud Logging
//	                                          (no bucket analogue needed)
//
// Both arms are gated on the same kaizen-experimentation:wafEnabled config
// toggle and share wafRateLimitPerIP / wafBlockedCountries.
package gcp

import (
	"fmt"
	"strings"

	"github.com/pulumi/pulumi-gcp/sdk/v8/go/gcp/cloudrunv2"
	"github.com/pulumi/pulumi-gcp/sdk/v8/go/gcp/compute"
	"github.com/pulumi/pulumi-gcp/sdk/v8/go/gcp/dns"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"

	kconfig "github.com/kaizen-experimentation/infra/pkg/config"
	runfactory "github.com/kaizen-experimentation/infra/pkg/gcp/compute"
	"github.com/kaizen-experimentation/infra/pkg/types"
)

// edgeRoute binds a ComputeOutputs service key to its public routing rule.
// Exactly one of hostRouted / catchAll / pathPrefix is set per route.
type edgeRoute struct {
	// key is the ComputeOutputs.ServiceEndpoints key ("m1", "m2-pipeline", …).
	key string
	// slug names the per-route edge resources (NEG, backend, invoker).
	slug string
	// hostRouted routes the assign.kaizen.{domain} host to this backend (M1).
	hostRouted bool
	// catchAll makes this backend the URL map default service (M6).
	catchAll bool
	// pathPrefix is the URL map path rule, e.g. "/api/*".
	pathPrefix string
	// awsParity marks rules that exist identically on the AWS ALB. The
	// remaining rules are the issue #496 all-8-services superset.
	awsParity bool
}

// edgeRoutes is the authoritative route table. The parameterized topology
// test (infra/test/edge_topology_test.go, #496 slice 2) pins the same table
// independently so silent edits here fail loudly there.
var edgeRoutes = []edgeRoute{
	{key: "m1", slug: "m1-assignment", hostRouted: true, awsParity: true},
	{key: "m5", slug: "m5-management", pathPrefix: "/api/*", awsParity: true},
	{key: "m7", slug: "m7-flags", pathPrefix: "/flags/*", awsParity: true},
	{key: "m6", slug: "m6-ui", catchAll: true, awsParity: true},
	{key: "m2-pipeline", slug: "m2-pipeline", pathPrefix: "/ingest/*"},
	{key: "m2-orch", slug: "m2-orchestration", pathPrefix: "/orchestration/*"},
	{key: "m3", slug: "m3-metrics", pathPrefix: "/metrics/*"},
	{key: "m4a", slug: "m4a-analysis", pathPrefix: "/analysis/*"},
}

// wafPreconfiguredRules maps the two AWS managed rule groups onto Cloud
// Armor preconfigured WAF rules (see the package parity table). Sensitivity
// 1 mirrors the conservative default posture of AWS managed rules.
var wafPreconfiguredRules = []struct {
	priority int
	slug     string
	ruleSet  string
}{
	{10, "xss", "xss-v33-stable"},
	{11, "lfi", "lfi-v33-stable"},
	{12, "rfi", "rfi-v33-stable"},
	{13, "rce", "rce-v33-stable"},
	{14, "scanner", "scannerdetection-v33-stable"},
	{15, "protocol", "protocolattack-v33-stable"},
	{16, "session", "sessionfixation-v33-stable"},
	{20, "sqli", "sqli-v33-stable"},
}

// EdgeBackends narrows the service map returned by NewCompute to what
// NewEdge consumes: service key → Cloud Run service name. Threading the
// resource's Name output (not a recomputed string) gives Pulumi the
// dependency edge that orders NEG creation after the Cloud Run service.
func EdgeBackends(svcs map[string]*runfactory.CloudRunService) map[string]pulumi.StringInput {
	out := make(map[string]pulumi.StringInput, len(svcs))
	for key, svc := range svcs {
		out[key] = svc.Service.Name
	}
	return out
}

// NewEdge provisions the GCP edge stage and returns the cross-provider
// types.EdgeOutputs contract:
//
//   - LoadBalancerDns — the LB's global anycast IP (GCLBs have no native
//     hostname; see types.EdgeOutputs docs).
//   - CertificateRef  — managed certificate self-link.
//   - HostedZoneId    — Cloud DNS managed zone name.
//
// backends maps every edgeRoutes key to the Cloud Run service name fronted
// by that route (see EdgeBackends). Extra keys (e.g. "preview-canary") are
// ignored — the canary is not part of the public edge.
func NewEdge(
	ctx *pulumi.Context,
	cfg *kconfig.Config,
	backends map[string]pulumi.StringInput,
) (types.EdgeOutputs, error) {
	if cfg.Domain == "" {
		return types.EdgeOutputs{}, fmt.Errorf(
			"gcp.NewEdge: cfg.Domain is required when cloudProvider=gcp reaches the edge stage " +
				"(set via `pulumi config set kaizen-experimentation:domain <domain>`)")
	}
	if cfg.GCPProjectID == "" {
		return types.EdgeOutputs{}, fmt.Errorf("gcp.NewEdge: cfg.GCPProjectID is required")
	}
	for _, route := range edgeRoutes {
		if _, ok := backends[route.key]; !ok {
			return types.EdgeOutputs{}, fmt.Errorf(
				"gcp.NewEdge: backends missing service key %q required by the edge route table", route.key)
		}
	}
	region := cfg.GCPRegion
	if region == "" {
		region = "us-central1"
	}

	prefix := fmt.Sprintf("kaizen-%s-edge", cfg.Environment)
	zoneName := fmt.Sprintf("kaizen.%s", cfg.Domain)
	assignHost := fmt.Sprintf("assign.%s", zoneName)
	apiHost := fmt.Sprintf("api.%s", zoneName)

	// ── Global anycast IP ───────────────────────────────────────────────
	addr, err := compute.NewGlobalAddress(ctx, fmt.Sprintf("%s-ip", prefix), &compute.GlobalAddressArgs{
		Name:        pulumi.Sprintf("%s-ip", prefix),
		AddressType: pulumi.String("EXTERNAL"),
	})
	if err != nil {
		return types.EdgeOutputs{}, fmt.Errorf("creating global address: %w", err)
	}

	// ── Cloud Armor (WAF parity, toggled like pkg/aws/waf) ──────────────
	var armor *compute.SecurityPolicy
	if cfg.WafEnabled {
		armor, err = newCloudArmorPolicy(ctx, cfg, prefix)
		if err != nil {
			return types.EdgeOutputs{}, err
		}
	}

	// ── Per-service serverless NEG + backend + LB invoker binding ───────
	backendByKey := make(map[string]*compute.BackendService, len(edgeRoutes))
	for _, route := range edgeRoutes {
		neg, err := compute.NewRegionNetworkEndpointGroup(ctx, fmt.Sprintf("%s-neg-%s", prefix, route.slug),
			&compute.RegionNetworkEndpointGroupArgs{
				Name:                pulumi.Sprintf("%s-neg-%s", prefix, route.slug),
				Region:              pulumi.String(region),
				NetworkEndpointType: pulumi.String("SERVERLESS"),
				CloudRun: &compute.RegionNetworkEndpointGroupCloudRunArgs{
					// ToStringOutput: the field is a *PtrInput; the map's
					// static StringInput type doesn't satisfy it directly.
					Service: backends[route.key].ToStringOutput(),
				},
			})
		if err != nil {
			return types.EdgeOutputs{}, fmt.Errorf("creating NEG for %s: %w", route.key, err)
		}

		beArgs := &compute.BackendServiceArgs{
			Name: pulumi.Sprintf("%s-be-%s", prefix, route.slug),
			// EXTERNAL_MANAGED selects the global external Application Load
			// Balancer (the envoy-based path that supports serverless NEGs
			// with Cloud Armor).
			LoadBalancingScheme: pulumi.String("EXTERNAL_MANAGED"),
			// Protocol is not used for serverless NEG backends (the LB
			// always reaches Cloud Run over HTTPS/HTTP2, so gRPC for M1/M7
			// works end-to-end); set for readability.
			Protocol: pulumi.String("HTTPS"),
			Backends: compute.BackendServiceBackendArray{
				&compute.BackendServiceBackendArgs{
					Group: neg.SelfLink,
				},
			},
			// Request logs to Cloud Logging — the ALB access-logs-to-S3
			// analogue (sampleRate 1.0 = every request, like ALB).
			LogConfig: &compute.BackendServiceLogConfigArgs{
				Enable:     pulumi.Bool(true),
				SampleRate: pulumi.Float64(1.0),
			},
		}
		if armor != nil {
			beArgs.SecurityPolicy = armor.SelfLink
		}
		be, err := compute.NewBackendService(ctx, fmt.Sprintf("%s-be-%s", prefix, route.slug), beArgs)
		if err != nil {
			return types.EdgeOutputs{}, fmt.Errorf("creating backend service for %s: %w", route.key, err)
		}
		backendByKey[route.key] = be

		// Unauthenticated invoke for LB-mediated traffic only — the services'
		// INGRESS_TRAFFIC_INTERNAL_LOAD_BALANCER setting rejects direct
		// public calls to the run.app URL.
		_, err = cloudrunv2.NewServiceIamMember(ctx, fmt.Sprintf("%s-invoker-%s", prefix, route.slug),
			&cloudrunv2.ServiceIamMemberArgs{
				Project:  pulumi.String(cfg.GCPProjectID),
				Location: pulumi.String(region),
				Name:     backends[route.key],
				Role:     pulumi.String("roles/run.invoker"),
				Member:   pulumi.String("allUsers"),
			})
		if err != nil {
			return types.EdgeOutputs{}, fmt.Errorf("creating LB invoker binding for %s: %w", route.key, err)
		}
	}

	// ── URL map (route table) ───────────────────────────────────────────
	var m1Backend, m6Backend *compute.BackendService
	pathRules := compute.URLMapPathMatcherPathRuleArray{}
	for _, route := range edgeRoutes {
		switch {
		case route.hostRouted:
			m1Backend = backendByKey[route.key]
		case route.catchAll:
			m6Backend = backendByKey[route.key]
		default:
			pathRules = append(pathRules, &compute.URLMapPathMatcherPathRuleArgs{
				Paths:   pulumi.StringArray{pulumi.String(route.pathPrefix)},
				Service: backendByKey[route.key].SelfLink,
			})
		}
	}

	urlMap, err := compute.NewURLMap(ctx, fmt.Sprintf("%s-urlmap", prefix), &compute.URLMapArgs{
		Name:           pulumi.Sprintf("%s-urlmap", prefix),
		DefaultService: m6Backend.SelfLink,
		HostRules: compute.URLMapHostRuleArray{
			&compute.URLMapHostRuleArgs{
				Hosts:       pulumi.StringArray{pulumi.String(assignHost)},
				PathMatcher: pulumi.String("assign"),
			},
			&compute.URLMapHostRuleArgs{
				Hosts:       pulumi.StringArray{pulumi.String("*")},
				PathMatcher: pulumi.String("default"),
			},
		},
		PathMatchers: compute.URLMapPathMatcherArray{
			// Host rule assign.kaizen.{domain} → M1, all paths — the ALB
			// host-header rule forwards every path on that host.
			&compute.URLMapPathMatcherArgs{
				Name:           pulumi.String("assign"),
				DefaultService: m1Backend.SelfLink,
			},
			&compute.URLMapPathMatcherArgs{
				Name:           pulumi.String("default"),
				DefaultService: m6Backend.SelfLink,
				PathRules:      pathRules,
			},
		},
	})
	if err != nil {
		return types.EdgeOutputs{}, fmt.Errorf("creating URL map: %w", err)
	}

	// ── Managed certificate + TLS floor ─────────────────────────────────
	cert, err := compute.NewManagedSslCertificate(ctx, fmt.Sprintf("%s-cert", prefix),
		&compute.ManagedSslCertificateArgs{
			Name: pulumi.Sprintf("%s-cert", prefix),
			Managed: &compute.ManagedSslCertificateManagedArgs{
				Domains: pulumi.StringArray{
					pulumi.String(zoneName),
					pulumi.String(assignHost),
					pulumi.String(apiHost),
				},
			},
		})
	if err != nil {
		return types.EdgeOutputs{}, fmt.Errorf("creating managed certificate: %w", err)
	}

	// TLS 1.2 floor — parity with the ALB's
	// ELBSecurityPolicy-TLS13-1-2-2021-06 listener policy.
	sslPolicy, err := compute.NewSSLPolicy(ctx, fmt.Sprintf("%s-ssl-policy", prefix), &compute.SSLPolicyArgs{
		Name:          pulumi.Sprintf("%s-ssl-policy", prefix),
		MinTlsVersion: pulumi.String("TLS_1_2"),
		Profile:       pulumi.String("MODERN"),
	})
	if err != nil {
		return types.EdgeOutputs{}, fmt.Errorf("creating SSL policy: %w", err)
	}

	// ── Frontends: HTTPS (443) + HTTP (80) → 301 redirect ───────────────
	httpsProxy, err := compute.NewTargetHttpsProxy(ctx, fmt.Sprintf("%s-https-proxy", prefix),
		&compute.TargetHttpsProxyArgs{
			Name:            pulumi.Sprintf("%s-https-proxy", prefix),
			UrlMap:          urlMap.SelfLink,
			SslCertificates: pulumi.StringArray{cert.SelfLink},
			SslPolicy:       sslPolicy.SelfLink,
		})
	if err != nil {
		return types.EdgeOutputs{}, fmt.Errorf("creating HTTPS proxy: %w", err)
	}
	_, err = compute.NewGlobalForwardingRule(ctx, fmt.Sprintf("%s-https-fr", prefix),
		&compute.GlobalForwardingRuleArgs{
			Name:                pulumi.Sprintf("%s-https-fr", prefix),
			IpAddress:           addr.Address,
			PortRange:           pulumi.String("443"),
			Target:              httpsProxy.SelfLink,
			LoadBalancingScheme: pulumi.String("EXTERNAL_MANAGED"),
		})
	if err != nil {
		return types.EdgeOutputs{}, fmt.Errorf("creating HTTPS forwarding rule: %w", err)
	}

	// Parity with the ALB HTTP listener's HTTP_301 redirect-to-HTTPS.
	redirectMap, err := compute.NewURLMap(ctx, fmt.Sprintf("%s-http-redirect", prefix), &compute.URLMapArgs{
		Name: pulumi.Sprintf("%s-http-redirect", prefix),
		DefaultUrlRedirect: &compute.URLMapDefaultUrlRedirectArgs{
			HttpsRedirect: pulumi.Bool(true),
			// MOVED_PERMANENTLY_DEFAULT = 301, the ALB redirect status.
			RedirectResponseCode: pulumi.String("MOVED_PERMANENTLY_DEFAULT"),
			StripQuery:           pulumi.Bool(false),
		},
	})
	if err != nil {
		return types.EdgeOutputs{}, fmt.Errorf("creating redirect URL map: %w", err)
	}
	httpProxy, err := compute.NewTargetHttpProxy(ctx, fmt.Sprintf("%s-http-proxy", prefix),
		&compute.TargetHttpProxyArgs{
			Name:   pulumi.Sprintf("%s-http-proxy", prefix),
			UrlMap: redirectMap.SelfLink,
		})
	if err != nil {
		return types.EdgeOutputs{}, fmt.Errorf("creating HTTP proxy: %w", err)
	}
	_, err = compute.NewGlobalForwardingRule(ctx, fmt.Sprintf("%s-http-fr", prefix),
		&compute.GlobalForwardingRuleArgs{
			Name:                pulumi.Sprintf("%s-http-fr", prefix),
			IpAddress:           addr.Address,
			PortRange:           pulumi.String("80"),
			Target:              httpProxy.SelfLink,
			LoadBalancingScheme: pulumi.String("EXTERNAL_MANAGED"),
		})
	if err != nil {
		return types.EdgeOutputs{}, fmt.Errorf("creating HTTP forwarding rule: %w", err)
	}

	// ── Cloud DNS: zone + the three A records the AWS arm aliases ───────
	zone, err := dns.NewManagedZone(ctx, fmt.Sprintf("%s-zone", prefix), &dns.ManagedZoneArgs{
		Name:        pulumi.Sprintf("%s-zone", prefix),
		DnsName:     pulumi.String(zoneName + "."),
		Description: pulumi.String("Kaizen public zone — parity with the Route 53 kaizen.{domain} zone"),
		Labels:      gcpLabels(cfg),
	})
	if err != nil {
		return types.EdgeOutputs{}, fmt.Errorf("creating managed zone: %w", err)
	}
	for _, rec := range []struct{ slug, fqdn string }{
		{"root", zoneName},
		{"assign", assignHost},
		{"api", apiHost},
	} {
		_, err := dns.NewRecordSet(ctx, fmt.Sprintf("%s-a-%s", prefix, rec.slug), &dns.RecordSetArgs{
			ManagedZone: zone.Name,
			Name:        pulumi.String(rec.fqdn + "."),
			Type:        pulumi.String("A"),
			Ttl:         pulumi.Int(300),
			Rrdatas:     pulumi.StringArray{addr.Address},
		})
		if err != nil {
			return types.EdgeOutputs{}, fmt.Errorf("creating A record %q: %w", rec.slug, err)
		}
	}

	ctx.Export("gclbIpAddress", addr.Address)
	ctx.Export("certificateRef", cert.SelfLink)
	ctx.Export("dnsZoneName", zone.Name)

	return types.EdgeOutputs{
		// GCLBs have no native hostname; the anycast IP is the documented
		// GCP interpretation of LoadBalancerDns (types.EdgeOutputs).
		LoadBalancerDns: addr.Address,
		CertificateRef:  cert.SelfLink,
		HostedZoneId:    zone.Name,
		// LoadBalancerArn / LoadBalancerArnSuffix stay zero-valued on GCP
		// per the types.EdgeOutputs contract.
	}, nil
}

// newCloudArmorPolicy builds the Cloud Armor security policy mirroring the
// AWS WAF v2 web ACL rule-for-rule (see the package doc parity table).
func newCloudArmorPolicy(ctx *pulumi.Context, cfg *kconfig.Config, prefix string) (*compute.SecurityPolicy, error) {
	matchAll := &compute.SecurityPolicyRuleMatchArgs{
		VersionedExpr: pulumi.String("SRC_IPS_V1"),
		Config: &compute.SecurityPolicyRuleMatchConfigArgs{
			SrcIpRanges: pulumi.StringArray{pulumi.String("*")},
		},
	}

	rules := compute.SecurityPolicyRuleTypeArray{
		// AWS parity: rate-limit-per-ip — block IPs above the threshold in
		// a 5-minute window. Cloud Armor throttle denies the excess with
		// 429 while the source stays above the limit.
		&compute.SecurityPolicyRuleTypeArgs{
			Action:      pulumi.String("throttle"),
			Priority:    pulumi.Int(1),
			Description: pulumi.String("rate-limit-per-ip (AWS WAF parity: rate-based statement)"),
			Match:       matchAll,
			RateLimitOptions: &compute.SecurityPolicyRuleRateLimitOptionsArgs{
				ConformAction: pulumi.String("allow"),
				ExceedAction:  pulumi.String("deny(429)"),
				EnforceOnKey:  pulumi.String("IP"),
				RateLimitThreshold: &compute.SecurityPolicyRuleRateLimitOptionsRateLimitThresholdArgs{
					Count:       pulumi.Int(cfg.WafRateLimitPerIP),
					IntervalSec: pulumi.Int(300),
				},
			},
		},
	}

	// AWS parity: optional geo-block.
	if len(cfg.WafBlockedCountries) > 0 {
		terms := make([]string, len(cfg.WafBlockedCountries))
		for i, country := range cfg.WafBlockedCountries {
			terms[i] = fmt.Sprintf("origin.region_code == '%s'", country)
		}
		rules = append(rules, &compute.SecurityPolicyRuleTypeArgs{
			Action:      pulumi.String("deny(403)"),
			Priority:    pulumi.Int(2),
			Description: pulumi.String("geo-block (AWS WAF parity: geo match statement)"),
			Match: &compute.SecurityPolicyRuleMatchArgs{
				Expr: &compute.SecurityPolicyRuleMatchExprArgs{
					Expression: pulumi.String(strings.Join(terms, " || ")),
				},
			},
		})
	}

	// AWS parity: managed Common + SQLi rule groups → preconfigured WAF.
	for _, pre := range wafPreconfiguredRules {
		rules = append(rules, &compute.SecurityPolicyRuleTypeArgs{
			Action:      pulumi.String("deny(403)"),
			Priority:    pulumi.Int(pre.priority),
			Description: pulumi.String(fmt.Sprintf("preconfigured-waf-%s (AWS WAF parity: managed rule groups)", pre.slug)),
			Match: &compute.SecurityPolicyRuleMatchArgs{
				Expr: &compute.SecurityPolicyRuleMatchExprArgs{
					Expression: pulumi.String(
						fmt.Sprintf("evaluatePreconfiguredWaf('%s', {'sensitivity': 1})", pre.ruleSet)),
				},
			},
		})
	}

	// AWS parity: WebAclDefaultActionAllow. Cloud Armor requires an
	// explicit match-all rule at the lowest priority (2147483647).
	rules = append(rules, &compute.SecurityPolicyRuleTypeArgs{
		Action:      pulumi.String("allow"),
		Priority:    pulumi.Int(2147483647),
		Description: pulumi.String("default allow (AWS WAF parity: default action)"),
		Match:       matchAll,
	})

	armor, err := compute.NewSecurityPolicy(ctx, fmt.Sprintf("%s-armor", prefix), &compute.SecurityPolicyArgs{
		Name:        pulumi.Sprintf("%s-armor", prefix),
		Description: pulumi.String("Kaizen edge WAF — Cloud Armor at AWS WAF v2 ruleset parity"),
		Rules:       rules,
	})
	if err != nil {
		return nil, fmt.Errorf("creating Cloud Armor policy: %w", err)
	}
	return armor, nil
}
