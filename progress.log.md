# Progress log — issue #502: M2 throughput test (100K events/sec via Redpanda on GCP)

Worker: `claude-web-502-20260706` (H1 claim on #502, expires 2026-07-07T02:00Z).
Charter: infra-3 (streaming), `docs/agents/registry/infra-3.md`.

## 2026-07-06

### Session plan (MODE: INIT)

1. Startup ritual: sync main, read CLAUDE.md + infra-3 charter, verify baseline.
2. Build the Phase 4 throughput harness: synthetic generator against M2's gRPC
   ingest, measurement at ingest acceptance / Redpanda offset advance /
   consumer lag, deterministic pass-fail gate.
3. Offline tests for the gate, justfile recipes, CI wiring, runbook.
4. Single PR, merge-ready (`Closes #502`); live steady-state run on the GCP
   dev stack recorded on the issue as acceptance follow-through.

### Deviations from the dispatch ritual

- **Branch name**: dispatch asked for `infra-3/test/m2-throughput-100k`; this
  claude-web session runs on the harness-generated ref
  `claude/issue-502-20260706-0216`, which cannot be renamed after launch.
  CLAUDE.md's branch-naming table tolerates the `claude/<slug>` family for
  exactly this case; ownership rides on PR metadata (`infra-3` label +
  conventional-commit title).
- **Baseline verification**: this executor's Bash allowlist denies `go`,
  `cargo`, and `just` (headless run, no interactive approver), so
  `just test-infra` and `cargo test -p experimentation-ingest` could not be
  executed in-session. Mitigation: branch is cut from `main` head `e4ec72a`
  (green at merge of #717), no Rust/Go/infra sources are modified by this
  change, and the PR's required CI runs both suites before merge.

### Findings that shaped the design

- Precedent stack is k6 gRPC + bash orchestrator (`scripts/loadtest_pipeline.{js,sh}`,
  `nightly-loadtest.yml` validates parse only). Followed it — no new compiled
  module, no new dependency manifests.
- `pipeline_service.proto` has batch RPCs for exposure/metric/qoe but only
  unary `IngestRewardEvent`; generator keeps the repo's canonical 40/30/15/15
  mix with rewards driven unary.
- The platform already defines the lag SLO: `PipelineConsumerLag` alerts at
  `kafka_consumer_group_lag > 100000` (`monitoring/prometheus/alerts.yml`).
  Gate default = alert parity, so a "pass" can never be a paging incident.
- Downstream consumer group on the loaded topics: `bandit-policy-service`
  (M4b ← `reward_events`, `crates/experimentation-policy/src/config.rs`).
- Redpanda Cloud connectivity is private (SASL/SCRAM-SHA-512 + TLS,
  `infra/pkg/streaming/redpanda.go`); full runs happen from inside the tenant
  VPC — GitHub runners have no route, so CI carries only the offline pieces
  (k6 parse check + gate unit tests).

### Work done

- `scripts/loadtest_m2_throughput.js` — k6 generator, batch gRPC, unique
  event_ids (dedup-safe), accounting summary for the gate.
- `scripts/m2_throughput_watch.py` — rpk-based offset/lag sampler + pass-fail
  gate evaluator (stdlib only; text parsing, version-stable).
- `scripts/loadtest_m2_throughput.sh` — orchestrator: preflight → sampler →
  k6 warmup+steady → drain → gate.
- `scripts/test_m2_throughput_gate.sh` — offline parser/verdict tests
  (9 scenarios), `just test-m2-throughput-gate`.
- justfile: `loadtest-m2-throughput`, `loadtest-m2-throughput-smoke`,
  `test-m2-throughput-gate`.
- `docs/runbooks/m2-throughput-loadtest.md` — GCP dev-stack procedure,
  threshold rationale per Redpanda Cloud sizing, CI wiring diff.
- CI wiring caveat: `.github/workflows/` edits are blocked for the App token
  (no `workflows` permission); the exact `nightly-loadtest.yml` diff is in the
  runbook for a follow-up commit.

### Follow-through (not closable from this session)

- Apply the `nightly-loadtest.yml` diff (runbook §CI wiring).
- Execute the live 5-minute run from inside the dev VPC with
  `REQUIRE_GROUPS=1`; attach `gate_report.json` to #502.
