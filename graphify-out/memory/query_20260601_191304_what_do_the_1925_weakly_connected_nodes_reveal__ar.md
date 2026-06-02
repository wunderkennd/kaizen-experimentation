---
type: "query"
date: "2026-06-01T19:13:04.043011+00:00"
question: "What do the 1925 weakly-connected nodes reveal? Are there documentation drift patterns the graph caught?"
contributor: "graphify"
source_nodes: ["adrs_007_sdk_provider", "adrs_007_resilient_provider", "adrs_007_experiment_provider", "ExperimentClient", "AssignmentProvider", "fallback"]
---

# Q: What do the 1925 weakly-connected nodes reveal? Are there documentation drift patterns the graph caught?

## Answer

Most weakly-connected nodes are graph artifacts (cross-crate use statements not captured by AST, in-file unit tests not visible). But ADR doc concept nodes revealed a clean pattern: ADRs about algorithms (011-030 with 0% weak) cleanly linked to code because the implementation file is named after the concept (avlm.rs ↔ ADR-015 AVLM). ADRs about architecture (006 workspace, 007 SDK abstraction, 001 language choice — all 75-100% weak) couldn't link because they describe concepts without a single namespace. Investigating these revealed FIVE WERE IMPLEMENTED IN CODE the graph just missed: ADR-005 STARTING/CONCLUDING states (state_machine.rs:6-79), ADR-006 PyO3/wasm-bindgen/cbindgen (Cargo features), ADR-008 guardrail_alerts Kafka topic (kafka.rs:3,70), ADR-009 LayerAllocation cooldown (bucket_reuse.rs:36-37), ADR-024 experimentation-ffi deletion (confirmed deleted). But ADR-007 'ResilientProvider' produced the actionable finding: ResilientProvider is defined as the API entry point in ADR-007, but ALL 5 SDKs (iOS, Android, server-go, web, server-python) implement it as ExperimentClient with a fallback parameter — and the web/server-go SDKs even cite 'ADR-007 fallback chain' in comments. The class exists with the wrong name. Three other docs (design_doc_v7, sdk_provider.mermaid, streaming-video-sequence.md) propagate the wrong name. Filed #589 to reconcile ADR-007 doc terminology with code reality. Inverse of #583 finding: there function was in wrong file; here function is in right file under wrong name.

## Source Nodes

- adrs_007_sdk_provider
- adrs_007_resilient_provider
- adrs_007_experiment_provider
- ExperimentClient
- AssignmentProvider
- fallback