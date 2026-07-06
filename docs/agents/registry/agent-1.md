---
type: Kaizen Module Agent
title: "Agent-1: Assignment Service (M1)"
description: Owns variant allocation, interleaving list construction, bandit arm delegation, and the client SDKs.
resource: https://github.com/wunderkennd/kaizen-experimentation/tree/main/crates/experimentation-assignment
tags: [module-agent, rust, assignment, sdks]
timestamp: 2026-07-04T12:00:00Z
id: agent-1
label: agent-1
executors: [claude-workflow, claude-web, multiclaude]
language: Rust
ports: [50051]
owned_paths:
  - crates/experimentation-assignment/
  - crates/experimentation-interleaving/
  - sdks/
depends_on: [agent-4, agent-5]
---

# Charter

You own Module 1 (Assignment Service) — all variant allocation, interleaving list
construction, bandit arm delegation, and client SDKs (`sdks/{server-go,server-python,android,ios,web}`).
Serves gRPC + HTTP JSON via tonic-web on port 50051. Crate dependencies:
experimentation-hash, experimentation-proto, experimentation-interleaving, experimentation-core.

## Standards

- `cargo test -p experimentation-assignment` before every PR.
- Hash parity: any change to bucketing logic must pass all 10K vectors in
  `test-vectors/hash_vectors.json` (`just test-hash`).
- p99 latency targets: < 5ms GetAssignment, < 15ms GetSlateAssignment.
- Every new RPC ships with tonic-web JSON mode for SDK compatibility.
- ADR-031 (ConnectRPC pilot) governs the transport migration — `connectrpc-build`
  drives codegen from `build.rs`; `buffa` pinned to 0.7.

## Contract-test obligations (consumer writes the test)

- M1 ↔ M4b: slate assignment roundtrip; LP-adjusted probabilities logged.
- M1 ↔ M5: switchback/META config compatibility via StreamConfigUpdates.

## Cross-agent dependencies

- [agent-4](/agent-4.md): `SelectArm` / `GetSlateAssignment` response contracts.
- [agent-5](/agent-5.md): `StreamConfigUpdates` carries new experiment-type configs.

## Work tracking

`gh issue list --label "agent-1" --state open` — comment on the Issue when starting;
PRs include `Closes #N`; add `blocked` label with an explanatory comment when stuck.
