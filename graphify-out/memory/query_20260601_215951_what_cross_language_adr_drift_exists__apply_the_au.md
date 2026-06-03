---
type: "query"
date: "2026-06-01T21:59:51.865407+00:00"
question: "What cross-language ADR drift exists? Apply the audit beyond crates+adrs to services+sdks+proto+docs/design+docs/guides."
contributor: "graphify"
source_nodes: ["adrs_025_m5_rust_port", "crates_experimentation_management", "services_management_auth", "services_management_mlrate", "services_management_fdr", "services_metrics_metricql", "crates_metricql"]
---

# Q: What cross-language ADR drift exists? Apply the audit beyond crates+adrs to services+sdks+proto+docs/design+docs/guides.

## Answer

Expanded scope to 594 files (added 296: sdks, services, proto, docs/design, docs/guides). FLAGSHIP FINDING: ADR-025 Rust M5 port is at Phase 2 of 4 with two gaps. (1) Phase 1 incomplete: ADR-025 explicitly specifies 'RBAC interceptor: tonic interceptor extracting auth context, enforcing 4-level role hierarchy' but Rust M5 has zero auth code; Go M5 still owns the auth/ package (212 lines: NewAuthInterceptor, streamAuthInterceptor, withAuthHeaders(email,role)). (2) Phase 3 not started: the whole rationale for the port was statistical orchestration (ADR-018 OnlineFdrController, ADR-015 Phase 2 MLRATE, ADR-020 Adaptive, ADR-019 Portfolio, ADR-021 Feedback Loops). None ported. Go M5 still owns mlrate/, fdr/, adaptive/, surrogate/, sequential/, guardrail/. CLAUDE.md says ADR-025 'executed'; ADR header says 'Proposed'; main.rs says 'Phase 2'. Three claims, three different statuses. Filed #590. NEGATIVE RESULTS: (a) Graphify ID collisions — Go and Rust files with same parent_dir + filename + entity collapse to one node (metricql_analyze_analyze appears in both crates and services). The 24 'cross-language edges' were artifacts of this, not real cross-references. (b) MetricQL Go↔Rust is at parity by design and is the codebase's gold standard — Rust files explicitly cite Go counterparts by file path (lexer.rs:2 references services/metrics/internal/metricql/lexer.go). M3 compiles, M5 validates — different responsibilities, shared spec. (c) SDK ↔ ADR-007 already covered by #589. The cross-language audit added 296 files for ~1.4M tokens and produced one P2 finding (ADR-025 port state) plus confirmation that two other patterns are healthy.

## Source Nodes

- adrs_025_m5_rust_port
- crates_experimentation_management
- services_management_auth
- services_management_mlrate
- services_management_fdr
- services_metrics_metricql
- crates_metricql