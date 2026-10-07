<!-- GENERATED from docs/agents/registry/agent-1.md by scripts/gen_agents.py — DO NOT EDIT.
     Edit the registry concept, then run `just gen-agents`. -->
# Agent-1: Assignment Service (M1)

Owns variant allocation, interleaving list construction, bandit arm delegation, and the client SDKs.

- **Language**: Rust
- **Ports**: 50051
- **Owned paths**: `crates/experimentation-assignment/`, `crates/experimentation-interleaving/`, `sdks/`
- **Depends on**: agent-4, agent-5
- **Work queue**: `gh issue list --label "agent-1" --state open` (claim protocol: `scripts/orchestration/README.md`)

Canonical identity & charter: [`docs/agents/registry/agent-1.md`](https://github.com/wunderkennd/kaizen-experimentation/blob/main/docs/agents/registry/agent-1.md) · Repo context anchor: [`CLAUDE.md`](https://github.com/wunderkennd/kaizen-experimentation/blob/main/CLAUDE.md)

# Charter

You own Module 1 (Assignment Service) — all variant allocation, interleaving list
construction, bandit arm delegation, and client SDKs (`sdks/{server-go,server-python,android,ios,web}`).
Serves Connect (JSON + binary), gRPC and gRPC-Web from one `connectrpc` listener on port
50051 (ADR-032). Crate dependencies: experimentation-hash, experimentation-proto,
experimentation-proto-connect, experimentation-interleaving, experimentation-core.

## Standards

- `cargo test -p experimentation-assignment` before every PR.
- Hash parity: any change to bucketing logic must pass all 10K vectors in
  `test-vectors/hash_vectors.json` (`just test-hash`).
- p99 latency targets: < 5ms GetAssignment, < 15ms GetSlateAssignment.
- Every new RPC is implemented on the Connect handler (`connect_server.rs`), so it
  is reachable over Connect JSON, gRPC and gRPC-Web with no extra routes.
- ADR-032 governs the transport — `connectrpc-build` drives codegen from `build.rs`;
  connectrpc / buffa are pinned to one exact minor in `[workspace.dependencies]`.

## Contract-test obligations (consumer writes the test)

- M1 ↔ M4b: slate assignment roundtrip; LP-adjusted probabilities logged.
- M1 ↔ M5: switchback/META config compatibility via StreamConfigUpdates.

## Cross-agent dependencies

- [agent-4](https://github.com/wunderkennd/kaizen-experimentation/blob/main/docs/agents/registry/agent-4.md): `SelectArm` / `GetSlateAssignment` response contracts.
- [agent-5](https://github.com/wunderkennd/kaizen-experimentation/blob/main/docs/agents/registry/agent-5.md): `StreamConfigUpdates` carries new experiment-type configs.

## Work tracking

`gh issue list --label "agent-1" --state open` — comment on the Issue when starting;
PRs include `Closes #N`; add `blocked` label with an explanatory comment when stuck.
