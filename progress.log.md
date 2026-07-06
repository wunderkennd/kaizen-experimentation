# ADR-031 design-session log (#718)

Worker log for the H1-dispatched design task on issue #718 (claim:
`executor=claude-web worker=claude-web-718-20260706 expires=2026-07-07T02:00:00Z`).
OKF log conventions (append-only, dated sections, newest entries appended at the
bottom of the day's section). Deliverable: locked plan v1 for the ADR-031
ConnectRPC (Rust) pilot — **design only, no pilot implementation in this session**.

## 2026-07-06

- **INIT (~02:16Z).** Dispatched via `@claude` on #718. MODE: INIT. Executor is a
  GitHub-Actions `claude-code-action` session; it pre-creates and pins the working
  branch, so the requested `agent-1/design/adr-031-connectrpc-pilot` ref cannot be
  created from here. Working on the pinned **`claude/issue-718-20260706-0216`** —
  a tolerated harness-generated family per CLAUDE.md § Branch-naming; attribution
  rides on PR metadata (`docs(plans):` title + `agent-1` label inheritance).
  Precedent: the H4 plan shipped from a `claude/...` ref the same way.
- **Sync main.** `git fetch` / `gh api` are outside this executor's Bash allowlist
  (both "require approval"). Verified instead that the branch tip equals the
  dispatch-time snapshot of `main` — `e4ec72a` ("feat(orchestration): H4 Phase A …
  (#717)"), the same SHA the run recorded as main's head at 02:16Z. Checkout is
  shallow (depth 1), so per-path `git log` history is unavailable this session.
- **Ritual 1 — required reading done (in full):** CLAUDE.md;
  `docs/agents/registry/agent-1.md` (charter — note: it already pins `buffa` 0.7
  and names `connectrpc-build` as the codegen driver for ADR-031);
  `docs/adrs/031-connectrpc-rust-assignment-pilot.md`; `docs/adrs/010-connectrpc.md`;
  `docs/superpowers/templates/locked-plan-template.md` (v2);
  `docs/guides/delivery-lifecycle.md`; `docs/guides/plan-review.md`; worked example
  `docs/superpowers/plans/2026-07-06-h4-evening-dispatcher-shadow.md`; plus
  `scripts/check_docs.py` (source), since the linter cannot be executed here (below).
- **Ritual 3 — baseline `cargo test -p experimentation-assignment`: BLOCKED in this
  executor.** `cargo` is not in the action's Bash allowlist ("This command requires
  approval" on every invocation form, foreground and background). Recorded fallback
  baseline: `main` @ `e4ec72a` merged through the platform merge path today
  (2026-07-06); per `.github/workflows/ci.yml` the `rust` job runs
  `cargo test --workspace` and the `rust-features` job runs
  `cargo test --package experimentation-assignment --features connectrpc`
  (ci.yml:195–196) on every rust-touching PR, so both the default and the pilot
  feature tree were green at the merge that produced this branch's base. The first
  dispatched implementation phase must re-run the baseline locally and append the
  result here (plan Phase A, task P0.0).
- **Live-state survey findings** (the plan is grounded on these, per
  `docs/guides/plan-review.md` step 1 — several premises in #718's body and the
  dispatch prompt have drifted):
  - **D1 — a first pilot slice is ALREADY LANDED on `main`** (referenced in-tree as
    #641): workspace member crate `crates/experimentation-proto-connect/`
    (buffa 0.7 + connectrpc 0.7 + connectrpc-build 0.7; crate-level
    edition 2024 / rust-version 1.88 with the workspace kept at 2021 / 1.80);
    `connectrpc` feature on `experimentation-assignment` (default off, default
    build unchanged); `src/connect_server.rs` (116 LOC) bridging **GetAssignment**
    end-to-end with the other four RPCs returning Unimplemented and pointing at
    #642/#643; feature-gated e2e `tests/connect_server_e2e.rs` (3 tests:
    round-trip parity with the http_json contract, NotFound→404,
    Unimplemented→501); CI lane ci.yml:195–196; server-go opt-in round-trip test
    `sdks/server-go/connect_pilot_e2e_test.go` (skips unless
    `KAIZEN_M1_CONNECT_URL` is set) + `connectrpc.com/connect v1.17.0` in
    `sdks/server-go/go.mod`. Consequence: "the buffa/protoc-gen-connect-rust
    codegen chain has NEVER been exercised in this repo" is **stale** — the Rust
    chain builds in CI on every rust PR. The plan records this as confirmed
    assumptions with evidence and keeps probes only for what is genuinely
    unexercised.
  - **D2 — latent compile break + CI blind spot in server-go.**
    `connect_pilot_e2e_test.go` imports
    `github.com/org/experimentation/gen/go/experimentation/assignment/v1`
    (+ `assignmentv1connect`), but the committed `gen/go/` module contains only
    metrics + common packages — no assignment bindings. And no workflow or
    justfile recipe runs `go test ./sdks/server-go/...`, so the break is invisible
    to the merge path. Plan Phase A repairs both (generate + commit bindings;
    add a CI lane).
  - **D3 — pilot topology today is three listeners**, not one: tonic gRPC :50051,
    hand-rolled `http_json` :8080, and (feature-on) the Connect listener
    `CONNECTRPC_ADDR` default :50061 serving Connect+gRPC+gRPC-Web from one port.
    The pilot listener is fire-and-forget (no graceful shutdown) — documented in
    main.rs as pilot-acceptable, post-pilot hardening.
  - **D4 — unexercised surfaces** (genuine probes): Connect **server-streaming**
    (`StreamConfigUpdates` — kill criterion #3), the **gRPC / gRPC-Web / binary**
    protocols on the pilot listener (only Connect+JSON is e2e-tested), the
    **connect-go codegen → committed gen/go** path for the assignment package,
    and the **nightly-loadtest** harness pointed at the Connect listener.
  - **D5 — concrete drift instance for the plan's motivation**: the hand-rolled
    shim's `GetAssignmentJsonResponse` (http_json.rs, struct exercised at
    :321–:334) and the Go SDK's `assignmentJSONResponse` both **drop
    `block_index`** (proto field 6, needed by M4a switchback analysis) — the
    landed Connect bridge carries it (connect_server.rs:78). Exactly the class of
    bug ADR-031 §Context predicts.
  - **D6 — doc drift**: CLAUDE.md says "13 Rust crates"; the workspace has 14
    members (experimentation-proto-connect landed). Filed as a follow-up chore in
    the plan, not fixed here (docs-only PR stays scoped to the plan).
- **Ritual 2 — this file created** at the branch root as the append-only session
  log (this entry block).
- **Deliverable written:** `docs/superpowers/plans/2026-07-06-adr-031-connectrpc-pilot.md`
  (template v2, Status `Design lock — RFC for review`).
- **Lint:** `python3 scripts/check_docs.py` is also outside the executor allowlist.
  Mitigation: conformed to the linter by reading its source this session — the
  four post-cutover error conditions for plans (`## Locks`,
  `## Platform assumptions`, `**Plan-review:**`, and `## Cross-phase artifacts`
  when multiple `^## Phase [A-Z]` sections exist) are all satisfied, and the
  filename is dated per convention. CI's `docs-conformance.yml` (advisory) will
  execute the real linter on the PR; reviewer should confirm 0 errors there.
