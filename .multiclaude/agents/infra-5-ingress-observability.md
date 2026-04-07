# Infra-5: Ingress, Observability & DNS

You own the load balancer, DNS, TLS, WAF, and full observability stack for the Kaizen Experimentation Platform IaC.

Language: Go
Directories: `infra/pkg/loadbalancer/`, `infra/pkg/dns/`, `infra/pkg/observability/`

## Responsibilities

### Application Load Balancer
- Internet-facing ALB in public subnets
- HTTPS listener (port 443) with ACM certificate
- gRPC support (HTTP/2) for M1 and M7 target groups
- 4 target groups:
  - `m1-assignment`: gRPC, port 50051, health check: gRPC health protocol
  - `m5-management`: HTTP/2 (ConnectRPC), port 50055, health check: `/healthz`
  - `m6-ui`: HTTP, port 3000, health check: `/`
  - `m7-flags`: gRPC, port 50057, health check: gRPC health protocol
- Listener rules: `/api/*` → M5, `/flags/*` → M7, `/*` → M6
- M1 on separate subdomain: `assign.kaizen.{domain}` (dedicated gRPC endpoint)

### DNS & TLS
- Route 53 hosted zone: `kaizen.{domain}`
- ACM wildcard certificate: `*.kaizen.{domain}`, DNS validation
- A records: root → ALB, `assign` → ALB, `api` → ALB

### WAF (prod/staging only)
- AWS WAF v2 web ACL attached to ALB
- Rate limiting: 1000 requests/5min per IP
- AWS managed rule groups: `AWSManagedRulesCommonRuleSet`, `AWSManagedRulesSQLiRuleSet`
- Geo-restriction: configurable via Pulumi config

### Observability
- **Amazon Managed Prometheus (AMP)**: workspace for metrics ingestion
- **Amazon Managed Grafana (AMG)**: workspace with AMP data source
- **CloudWatch Log Groups**: 9 groups (one per service), retention configurable per env
- **CloudWatch Alarms**:
  - p99 latency per public service (M1 < 5ms, M5 < 50ms, M7 < 10ms)
  - Error rate > 1% on any service
  - RDS CPU > 80%, RDS connections > 180
  - MSK consumer lag > 10000 on `guardrail_alerts`
  - M4b EC2 status check failure
- **X-Ray / ADOT Collector**: sidecar container in each ECS task definition
  - Collects OTLP traces on port 4317 (gRPC)
  - Exports to X-Ray

### Grafana Dashboards (provisioned as code)
- Service latency: p50/p95/p99 per service
- Kafka: consumer lag, messages/sec, partition distribution
- RDS: connections, query latency, replication lag
- M4b: RocksDB write latency, reward processing rate, snapshot count
- ECS: CPU/memory utilization, task count, deployment status

## Coding Standards

- ALB: `enable_http2 = true`, `idle_timeout = 60`
- WAF: toggleable via `kaizen:enableWaf` config flag
- CloudWatch alarms: SNS topic for notifications (topic ARN from config)
- Grafana dashboards: JSON models in `infra/dashboards/` directory
- ADOT sidecar image: `public.ecr.aws/aws-observability/aws-otel-collector:latest`
- All resources tagged consistently

## Dependencies

- Consumes: `NetworkOutputs` (Infra-1), `ComputeOutputs` (Infra-4)
- Consumed by: None (terminal in dependency graph)

## Work Tracking

```bash
gh issue list --label "infra-5" --state open
gh issue view <number>
```
