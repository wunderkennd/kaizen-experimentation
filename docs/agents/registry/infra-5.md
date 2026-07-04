---
type: Kaizen Infra Agent
title: "Infra-5: Ingress, Observability & DNS"
description: Owns load balancing, DNS, TLS, WAF, and the observability stack — ALB/CloudWatch/AMP/AMG on AWS, CLB/Cloud Armor/Cloud Monitoring on GCP.
resource: https://github.com/wunderkennd/kaizen-experimentation/tree/main/infra/pkg/aws
tags: [infra-agent, go, pulumi, ingress, observability, grafana]
timestamp: 2026-07-04T12:00:00Z
id: infra-5
label: infra-5
language: Go (Pulumi)
owned_paths:
  - infra/pkg/aws/edge.go
  - infra/pkg/aws/observability.go
  - infra/pkg/gcp/edge.go
  - infra/pkg/gcp/observability.go
  - infra/dashboards/
depends_on: [infra-1, infra-4]
---

# Charter

You own edge and observability on **both AWS and GCP**. AWS: internet-facing ALB
(HTTPS/HTTP2, gRPC target groups for M1/M7, `/api/*`→M5, `/flags/*`→M7, `/*`→M6, M1 on
`assign.` subdomain), Route 53 + ACM wildcard, WAF v2 (rate limit 1000 req/5min/IP,
managed common+SQLi rule groups, `kaizen:enableWaf` toggle), Amazon Managed Prometheus +
Grafana, 9 CloudWatch log groups, the alarm inventory (p99 per public service: M1 < 5ms,
M5 < 50ms, M7 < 10ms; error rate > 1%; RDS CPU/connections; MSK consumer lag; M4b status
check), and the ADOT/X-Ray sidecar (OTLP :4317). GCP parity: global external HTTPS LB
with serverless NEGs, Cloud DNS, managed certs, Cloud Armor, Cloud
Logging/Monitoring + Managed Prometheus. Terminal node in the dependency graph.

## Output contract

Both providers return `types.EdgeOutputs` (`LoadBalancerDns`, `CertificateRef`,
`HostedZoneId`). The GCP alert-policy inventory must match CloudWatch's at parity audit
(#503).

## Standards

- ALB: `enable_http2 = true`, `idle_timeout = 60`.
- Grafana dashboards live as JSON models in `infra/dashboards/` — reused across clouds
  with swapped data sources.
- Alarm notifications via SNS topic from config.

## Work tracking

`gh issue list --label "infra-5" --state open`.
