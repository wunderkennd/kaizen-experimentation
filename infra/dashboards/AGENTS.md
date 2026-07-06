<!-- GENERATED from docs/agents/registry/infra-5.md by scripts/gen_agents.py — DO NOT EDIT.
     Edit the registry concept, then run `just gen-agents`. -->
# Infra-5: Ingress, Observability & DNS

Owns load balancing, DNS, TLS, WAF, and the observability stack — ALB/CloudWatch/AMP/AMG on AWS, CLB/Cloud Armor/Cloud Monitoring on GCP.

- **Language**: Go (Pulumi)
- **Owned paths**: `infra/pkg/aws/edge.go`, `infra/pkg/aws/observability.go`, `infra/pkg/gcp/edge.go`, `infra/pkg/gcp/observability.go`, `infra/dashboards/`
- **Depends on**: infra-1, infra-4
- **Work queue**: `gh issue list --label "infra-5" --state open` (claim protocol: `scripts/orchestration/README.md`)

Canonical identity & charter: [`docs/agents/registry/infra-5.md`](https://github.com/wunderkennd/kaizen-experimentation/blob/main/docs/agents/registry/infra-5.md) · Repo context anchor: [`CLAUDE.md`](https://github.com/wunderkennd/kaizen-experimentation/blob/main/CLAUDE.md)

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
