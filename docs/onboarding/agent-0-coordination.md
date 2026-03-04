# Cross-Agent Coordination Guide

## Dependency Map: Who Unblocks Whom

The critical path through the system runs: **Proto Schema → M1 Hash (Agent-1) → M2 Pipeline (Agent-2) → M3 Metrics (Agent-3) → M4a Analysis (Agent-4) → M6 UI (Agent-6)**. M5 Management (Agent-5) is a parallel critical path that unblocks both M1 (configs) and M6 (APIs).

### Phase 1 Integration Order

```
Week 2:  Agent-1 delivers experimentation-hash crate (unblocks Agent-7 CGo bridge)
         Agent-5 delivers Experiment CRUD APIs (unblocks Agent-6 experiment list)
Week 3:  Agent-2 delivers IngestExposure + IngestMetricEvent RPCs (unblocks Agent-3)
         Agent-1 delivers GetAssignment RPC (unblocks SDK development)
Week 4:  Agent-5 delivers StreamConfigUpdates (unblocks Agent-1 config cache)
         Agent-3 delivers standard metric computation (unblocks Agent-4 M4a)
Week 5:  Agent-4 delivers Welch t-test + SRM (unblocks Agent-6 results page)
         Agent-3 delivers guardrail breach detection (unblocks Agent-5 auto-pause)
Week 6:  Agent-6 delivers experiment list + results dashboard (first stakeholder demo)
         Agent-7 delivers flag CRUD + PromoteToExperiment (feature completeness)
```

### Who Blocks Whom (agent-to-agent)

| If this agent is late... | These agents are impacted... | Severity | Workaround |
|--------------------------|------------------------------|----------|------------|
| Agent-1 (Hash) | Agent-7 (CGo bridge), all SDKs | HIGH | Agent-7 uses a pure-Go MurmurHash3 temporarily (replace later with CGo) |
| Agent-2 (Pipeline) | Agent-3 (no events to compute), Agent-4 M4b (no rewards) | **CRITICAL** | Agent-3 and Agent-4 use synthetic data generators |
| Agent-3 (Metrics) | Agent-4 M4a (no metric summaries to analyze) | **CRITICAL** | Agent-4 uses hand-crafted Parquet files |
| Agent-4 (Analysis) | Agent-6 (no results to display), Agent-5 (no auto-conclude signal) | HIGH | Agent-6 mocks API responses with static JSON |
| Agent-5 (Management) | Agent-1 (no configs), Agent-6 (no CRUD APIs), Agent-3 (no experiment list) | **CRITICAL** | All downstream agents use local JSON config files |
| Agent-6 (UI) | Nobody | LOW | Platform works without UI; just less visible |
| Agent-7 (Flags) | Nobody directly | LOW | Flag functionality is additive |

### Mock Contracts

Every agent should define mock implementations for their downstream dependencies on Day 1:

- **Agent-1**: Mock M5 config with a local JSON file. Mock M4b SelectArm with uniform random.
- **Agent-2**: Generate synthetic events via a Go/Rust script. No upstream dependency.
- **Agent-3**: Generate synthetic Kafka events. Mock M5 with local experiment config.
- **Agent-4 M4a**: Generate synthetic metric_summaries as Parquet files. No live M3 dependency.
- **Agent-4 M4b**: Generate synthetic reward events on Kafka. No live M2 dependency.
- **Agent-5**: No upstream mocks needed (PostgreSQL is your only dependency).
- **Agent-6**: MSW (Mock Service Worker) mocking all ConnectRPC responses.
- **Agent-7**: Mock M5 CreateExperiment as a no-op. Mock hash with pure-Go MurmurHash3 if CGo bridge isn't ready.

## Integration Testing Protocol

### Integration Test Environments

| Environment | Purpose | When |
|-------------|---------|------|
| **Agent-local** | Single agent + mocked dependencies (Docker Compose) | Every PR |
| **Pair integration** | Two agents communicating (e.g., M2→M3, M5→M1) | Weekly, starting Week 3 |
| **Full stack** | All 7 modules + infra (staging) | Weekly, starting Week 6 |

### Pair Integration Schedule

```
Week 3:  Agent-5 ↔ Agent-6 (management API + UI)
         Agent-1 ↔ Agent-5 (config streaming)
Week 4:  Agent-2 ↔ Agent-3 (event pipeline → metric computation)
         Agent-1 ↔ Agent-7 (hash parity via CGo)
Week 5:  Agent-3 ↔ Agent-4 (metric summaries → analysis)
         Agent-5 ↔ Agent-3 (guardrail alerts → auto-pause)
Week 6:  Agent-1 ↔ Agent-4 (bandit delegation: assignment → SelectArm)
         Agent-4 ↔ Agent-6 (analysis results → UI rendering)
```

## Communication Protocol

### Async Communication (default)
- **Proto changes**: PR against proto/ directory. All affected agents review. buf breaking CI enforces backward compatibility.
- **Schema changes** (PostgreSQL, Delta Lake, Kafka): PR with migration script. Affected agents review.
- **ADR updates**: PR with updated ADR markdown. Agents affected by the decision review.

### Sync Communication (escalation)
- **Blocking dependency**: If you're blocked on another agent's deliverable, open a GitHub issue tagged `blocking` and ping in Slack. Expected response: 4 hours.
- **Contract disagreement**: If two agents disagree on an API contract, escalate to a 30-minute design sync. Decision documented in a new ADR.
- **Integration failure**: If pair integration fails, both agents meet within 24 hours to debug. Root cause documented in the PR.

### Weekly Sync
- **Monday**: Each agent posts a 3-line status update: (1) what shipped last week, (2) what's planned this week, (3) any blockers.
- **Thursday**: 30-minute all-agent sync to review pair integration results and adjust the following week's plan.

## Shared Development Infrastructure

All agents share these resources:

| Resource | Location | Owner |
|----------|----------|-------|
| Proto schema | `proto/` directory | Shared (any agent can propose changes) |
| PostgreSQL migrations | `sql/migrations/` | Agent-5 (M5) owns schema; others submit PRs |
| Delta Lake table definitions | `delta/` | Agent-3 (M3) owns schema; Agent-4 reads |
| Kafka topic configs | `kafka/` | Agent-2 (M2) owns producer config; consumers own consumer config |
| Docker Compose (local dev) | `docker-compose.yml` | Shared |
| CI/CD pipeline | `.github/workflows/` | Shared |
| Hash test vectors | `test-vectors/hash_vectors.json` | Agent-1 generates; all agents validate |

## Definition of Done (per Phase Gate)

An agent's phase deliverable is "done" when:

1. All acceptance criteria from the design doc pass.
2. Unit test coverage > 90% for new code.
3. Integration test with at least one downstream consumer passes.
4. Documentation updated (README, API docs, this onboarding guide if needed).
5. PR reviewed and merged to main.
6. Docker image builds and deploys to staging.
7. Pair integration test with primary downstream consumer succeeds.
