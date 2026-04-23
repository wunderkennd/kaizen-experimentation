# Kaizen Experimentation — Customer Integration Documentation Outline

**Status**: Draft outline (structure only — sections to be authored)
**Audience**: External/partner engineering teams integrating Kaizen into an SVOD or similar product surface
**Scope**: End-to-end path from "zero credentials" to "running a live experiment with trusted analysis"

---

## 0. How to Use This Documentation

- 0.1 Reading paths by role
  - 0.1.1 Product Manager — "I want to ship an experiment"
  - 0.1.2 Client Engineer (iOS / Android / Web) — "I need to fetch variants"
  - 0.1.3 Backend Engineer — "I need to emit exposure + metric events"
  - 0.1.4 Data / Analytics Engineer — "I need to pipe my metrics in and read results out"
  - 0.1.5 Platform / DevOps — "I need to deploy or self-host"
- 0.2 Documentation conventions (code blocks, callouts, stability badges)
- 0.3 Version matrix (platform ↔ SDK ↔ proto compatibility)
- 0.4 Where to get help (Slack, issue tracker, on-call escalation)

---

## 1. Introduction

- 1.1 What Kaizen is
  - SVOD-grade experimentation + feature flag platform
  - Schema-first (Protobuf) across 7 modules
- 1.2 What Kaizen is not
  - Not a product analytics tool (BI layer lives elsewhere)
  - Not a CDP / identity graph
- 1.3 Core capabilities at a glance
  - A/B, A/B/n, multivariate, factorial
  - Interleaving (Team Draft, Optimized, Multileave)
  - Multi-armed and contextual bandits (Thompson, LinUCB, Neural)
  - Quasi-experimental (switchback, synthetic control)
  - Feature flags with percentage rollouts
  - Sequential / always-valid inference (AVLM, e-values, mSPRT)
- 1.4 Platform concepts in one page (diagram + glossary)
- 1.5 When to choose Kaizen vs. a lighter alternative

---

## 2. Core Concepts

- 2.1 Entities
  - 2.1.1 Experiment, Variant, Treatment, Holdout
  - 2.1.2 Assignment Unit (user, device, session, account, household)
  - 2.1.3 Metric, Guardrail, Decision Criterion
  - 2.1.4 Feature Flag vs. Experiment (and when a flag graduates)
- 2.2 Lifecycle states
  - `draft` → `review` → `running` → `analyzing` → `decided` → `archived`
- 2.3 Bucketing model
  - MurmurHash3 + salt; why it's deterministic and reproducible
  - Bucket reuse, carryover, and hash salt rotation
- 2.4 Exposure vs. enrollment vs. trigger events
- 2.5 Guardrails and stop conditions
- 2.6 Privacy and PII boundaries (what you may and may not send)

---

## 3. Architecture Overview (Customer-Facing)

- 3.1 Module map (M1–M7) and which module a customer talks to
  - M1 Assignment (50051) — variant allocation
  - M2 Pipeline (50052 / 50058) — event ingestion
  - M3 Metrics (50056) — metric computation (usually indirect)
  - M4a Analysis (50053) — statistical results
  - M4b Bandit (50054) — bandit arm selection
  - M5 Management (50055) — CRUD / lifecycle / RBAC
  - M6 UI (3000) — console
  - M7 Flags (50057) — feature flag evaluation
- 3.2 Data flow: client → assignment → exposure → pipeline → metrics → analysis → decision
- 3.3 Deployment topologies
  - Managed (SaaS)
  - Self-hosted (on your Kubernetes / Nomad)
  - Hybrid (edge assignment + central analytics)
- 3.4 SLAs, regions, and latency expectations

---

## 4. Getting Started (15-Minute Quickstart)

- 4.1 Prerequisites (workspace, API key, SDK environment)
- 4.2 Create your first experiment via the UI
- 4.3 Install an SDK (pick one)
- 4.4 Fetch your first assignment
- 4.5 Emit your first exposure event
- 4.6 View assignment counts in the console
- 4.7 "Hello, experiment" sanity checklist
- 4.8 Common first-time errors and fixes

---

## 5. Authentication, Authorization, and Tenancy

- 5.1 Workspaces, projects, environments (`dev` / `staging` / `prod`)
- 5.2 API keys vs. OAuth client credentials vs. mTLS
- 5.3 Roles and RBAC matrix (Viewer, Analyst, Experimenter, Admin, Owner)
- 5.4 Service accounts for server-side SDKs
- 5.5 Secret storage best practices
- 5.6 Audit log access
- 5.7 SSO / SAML / SCIM (enterprise)

---

## 6. SDK Integration Guides

Each SDK chapter follows the same template: install → initialize → assign → expose → flag → shutdown → troubleshoot.

- 6.1 Web SDK (TypeScript / React)
  - Client-side vs. SSR assignment
  - Hydration safety, flicker mitigation
  - Bundle size and lazy evaluation
- 6.2 iOS SDK (Swift)
  - App launch path, cold-start budget
  - Backgrounding, offline queue
  - App Store review considerations
- 6.3 Android SDK (Kotlin)
  - Application vs. Activity scope
  - WorkManager integration for event flush
- 6.4 Server-side Go SDK
  - Context propagation, deadlines
  - gRPC connection pooling
- 6.5 Server-side Python SDK
  - asyncio vs. sync clients
  - Framework recipes (FastAPI, Django, Flask)
- 6.6 Direct gRPC / Protobuf (no SDK)
  - Proto imports, buf module usage
  - Wire-format contract tests
- 6.7 Edge / CDN integration (Cloudflare Workers, Fastly, Akamai)

---

## 7. Creating and Managing Experiments

- 7.1 Designing a good experiment (brief checklist)
- 7.2 UI walkthrough (screenshots + narration)
- 7.3 Programmatic creation via Management API
  - `ExperimentService.Create` end-to-end example
  - Schema reference (proto → field-by-field)
- 7.4 Targeting and audiences
  - Attribute-based targeting
  - Mutual exclusion groups
  - Layers and holdouts
- 7.5 Traffic allocation
  - Ramp plans, gradual rollout
  - Automatic ramp gates
- 7.6 Metrics and guardrails
  - Declaring primary, secondary, guardrail metrics
  - Minimum detectable effect and power calculations
- 7.7 Reviewing, approving, and launching
- 7.8 Editing a live experiment (what you can and cannot change)
- 7.9 Stopping, archiving, and decisioning

---

## 8. Feature Flags (M7)

- 8.1 Flags vs. experiments — when to use which
- 8.2 Flag types (boolean, string, JSON, number)
- 8.3 Percentage rollouts and sticky bucketing
- 8.4 Targeting rules and rule order
- 8.5 Kill switches and emergency rollback
- 8.6 Graduating a flag from experiment to permanent
- 8.7 Flag hygiene and stale-flag reports

---

## 9. Event Ingestion (M2 Pipeline)

- 9.1 Event taxonomy (exposure, metric, conversion, attribute)
- 9.2 Required fields and schemas
- 9.3 Event validation and common rejection reasons
- 9.4 Deduplication (Bloom filter guarantees)
- 9.5 Delivery modes
  - SDK-managed batching
  - Direct Kafka producer
  - HTTP event collector
  - Server-side CDP forwarding (Segment, Rudderstack, mParticle)
- 9.6 Throughput, backpressure, and rate limits
- 9.7 Late-arriving events and reprocessing

---

## 10. Metrics (M3)

- 10.1 Metric definitions (count, ratio, funnel, revenue, engagement)
- 10.2 Custom metric SQL and the metric registry
- 10.3 Metric freshness and latency expectations
- 10.4 Pre-computed metric tables and Delta Lake layout
- 10.5 Bringing your own warehouse (BYOW) — federated read patterns
- 10.6 Metric governance (owners, deprecation, versioning)

---

## 11. Analysis and Results (M4a)

- 11.1 Reading a results dashboard
- 11.2 Statistical methods available
  - Frequentist t-test, Welch, Mann–Whitney
  - CUPED and AVLM (sequential CUPED)
  - Group sequential tests (GST)
  - e-values + online FDR
  - Synthetic control and switchback estimators
- 11.3 Choosing the right test (decision tree)
- 11.4 Interpreting confidence intervals, p-values, and posteriors
- 11.5 Peeking safely — always-valid vs. fixed-horizon
- 11.6 Heterogeneous treatment effects (HTE) and segments
- 11.7 Exporting results (CSV, API, webhook)

---

## 12. Bandits and Adaptive Experiments (M4b)

- 12.1 When to use bandits instead of A/B
- 12.2 Algorithms offered (Thompson, LinUCB, Neural, Slate)
- 12.3 Reward design and reward latency
- 12.4 Contextual features and feature stores
- 12.5 Cold-start strategies
- 12.6 Multi-objective and constrained bandits
- 12.7 Offline evaluation and surrogate calibration
- 12.8 Bandit-specific monitoring

---

## 13. Advanced Experimental Designs

- 13.1 Interleaving (ranking experiments)
- 13.2 Switchback experiments (marketplace / two-sided)
- 13.3 Synthetic control (geo experiments)
- 13.4 Factorial / multivariate designs
- 13.5 Meta-experiments and portfolio optimization
- 13.6 Interference and spillover mitigation

---

## 14. Operational Integration

- 14.1 Observability
  - OpenTelemetry traces emitted by SDKs
  - Recommended dashboards (Grafana templates)
  - Alert hooks (PagerDuty, Opsgenie)
- 14.2 Data residency and regional deployments
- 14.3 Disaster recovery: what survives a Kaizen outage
  - Graceful degradation in each SDK
  - Local assignment fallback cache
- 14.4 Capacity planning cheatsheet (QPS per module)
- 14.5 Webhooks and event subscriptions

---

## 15. Governance, Security, and Compliance

- 15.1 Data classification and handling
- 15.2 PII redaction and hashing requirements
- 15.3 GDPR / CCPA / LGPD user deletion flows
- 15.4 SOC 2, ISO 27001 posture (summary)
- 15.5 Change management and audit trails
- 15.6 Approval workflows for high-risk experiments

---

## 16. Reference

- 16.1 gRPC / Protobuf API reference (generated)
- 16.2 REST / JSON gateway reference (if applicable)
- 16.3 SDK API reference per language
- 16.4 Error code catalog
- 16.5 Rate limits and quotas
- 16.6 Glossary
- 16.7 Proto schema stability policy

---

## 17. Cookbook (Task-Oriented Recipes)

- 17.1 Run an A/B on a pricing page
- 17.2 Ramp a risky feature to 1% → 10% → 50% → 100%
- 17.3 Instrument a conversion funnel
- 17.4 Add a guardrail on streaming error rate
- 17.5 Convert a winning experiment to a permanent flag
- 17.6 Reuse a bucket hash across experiments safely
- 17.7 Run a geo experiment with synthetic control
- 17.8 Multi-objective bandit for homepage ranking
- 17.9 Backfill historical exposures
- 17.10 Migrate from Optimizely / LaunchDarkly / Statsig / Eppo

---

## 18. Troubleshooting and FAQ

- 18.1 "Why is my user getting a different variant on web vs. iOS?"
- 18.2 "Why is my sample ratio mismatch (SRM) alert firing?"
- 18.3 "Why do exposure counts not match my analytics tool?"
- 18.4 "My p-value is jumping — is that normal?"
- 18.5 "Results dashboard shows 'insufficient power'"
- 18.6 SDK connection, retry, and fallback behaviors
- 18.7 Common proto / wire format compatibility errors

---

## 19. Release Notes and Deprecation Policy

- 19.1 Platform release channel (stable, beta, preview)
- 19.2 SDK versioning (semver, LTS lines)
- 19.3 Breaking change policy and notice periods
- 19.4 Current deprecations and migration guides

---

## 20. Appendices

- 20.1 Example repositories (per SDK, runnable end-to-end)
- 20.2 Postman / Bruno collection for the management API
- 20.3 Buf module reference and codegen instructions
- 20.4 Reference architectures (SVOD, commerce, marketplace)
- 20.5 Third-party integrations matrix (CDP, warehouse, BI, IdP)
- 20.6 Support SLAs and escalation paths

---

## Appendix A — Authoring Notes (internal, strip before publishing)

- Every chapter begins with a 3-bullet "What you'll learn" and ends with "Next steps".
- Every code example must be runnable and live in `examples/` with CI coverage.
- Golden-file-validated statistical claims must link to the corresponding ADR and reference package (per `CLAUDE.md` table).
- Proto references should link to `proto/experimentation/<module>/v1/` and include the buf module URL.
- SDK chapters must share an identical skeleton so customers can diff languages.
- Every "how to" recipe in §17 must map to at least one entity and state in §2.
