# Agent-5 Quickstart: M5 Experiment Management Service (Go)

## Your Identity

| Field | Value |
|-------|-------|
| Module | M5: Experiment Management Service |
| Language | Go |
| Go packages you own | `services/management/` |
| Proto package | `experimentation.management.v1` |
| Infra you own | PostgreSQL config tables, audit trail |
| Primary SLA | API p99 < 100ms, state transitions atomic (no partial state), auto-pause < 60s from guardrail alert |

## Read These First (in order)

1. **Design doc v5.1** — Sections 9 (M5 specification), 2.5 (component state machine), 2.9 (bucket reuse), 2.10 (guardrails default to auto-pause), 9.3 (experiment type validation table)
2. **ADR-005** (component state machine), **ADR-008** (auto-pause guardrails), **ADR-009** (bucket reuse)
3. **Proto files** — `management_service.proto`, `experiment.proto`, `layer.proto`, `metric.proto`, `targeting.proto`, `surrogate.proto`
4. **PostgreSQL DDL** — `sql/001_schema.sql` (you own: experiments, variants, layers, layer_allocations, guardrail_configs, targeting_rules, metric_definitions, surrogate_models, audit_trail)
5. **Mermaid diagram** — `state_machine.mermaid` (you enforce this lifecycle)

## You Are the Orchestrator

M5 is the central coordination point. You don't compute anything statistical — you manage experiment configuration, enforce lifecycle rules, and coordinate other services. Think of yourself as the "control plane" while M1/M3/M4a/M4b are the "data plane."

## Who You Depend On (upstream)

| Module | What you need from them | Blocks you? |
|--------|------------------------|-------------|
| M4a (Agent-4) | Analysis results in PostgreSQL (for auto-conclude sequential experiments) | Partially — you trigger CONCLUDING, M4a runs analysis, then you transition to CONCLUDED. |
| M3 (Agent-3) | Guardrail alerts on Kafka `guardrail_alerts` topic | Yes for auto-pause feature. Mock with synthetic alerts initially. |
| PostgreSQL | Config and state storage | Yes — your primary data store. |

## Who Depends on You (downstream)

| Module | What they need from you | Impact if you're late |
|--------|------------------------|----------------------|
| M1 (Agent-1) | Experiment configs via `StreamConfigUpdates` | **Critical** — M1 needs configs to serve assignments. |
| M3 (Agent-3) | Experiment and metric definitions to know what to compute | **Critical** — M3 needs to know which experiments are RUNNING. |
| M4a (Agent-4) | CONCLUDING state trigger to begin final analysis | Analysis doesn't start without your signal. |
| M6 (Agent-6) | All CRUD APIs for the dashboard | **UI is empty without you.** |
| M7 (Agent-7) | `PromoteToExperiment` creates experiment from flag | Flag graduation blocked. |

## Your First PR: Experiment CRUD + State Machine

**Goal**: Create, read, update experiments with enforced state transitions and audit logging.

```
services/management/
├── cmd/
│   └── main.go                  # connect-go server
├── internal/
│   ├── handlers/
│   │   ├── experiment.go        # CRUD RPCs
│   │   ├── lifecycle.go         # Start, Conclude, Archive, Pause, Resume
│   │   ├── metric.go            # MetricDefinition CRUD
│   │   ├── layer.go             # Layer + allocation management
│   │   └── surrogate.go         # Surrogate model CRUD
│   ├── state/
│   │   └── machine.go           # Valid transitions, per-type validation gates
│   ├── allocation/
│   │   └── bucket.go            # Layer allocation + bucket reuse logic
│   ├── audit/
│   │   └── trail.go             # Log every mutation to audit_trail
│   └── validation/
│       ├── experiment.go        # Type-specific config validation
│       └── guardrail.go         # Guardrail threshold validation
```

**Acceptance criteria**:
- `CreateExperiment` → state = DRAFT, hash_salt auto-generated, audit_trail entry created.
- `StartExperiment` on DRAFT → state = STARTING → (async validation) → state = RUNNING. Audit trail records both transitions.
- `StartExperiment` on RUNNING → gRPC error FAILED_PRECONDITION ("experiment already running").
- `ConcludeExperiment` on RUNNING → state = CONCLUDING → (triggers M4a final analysis) → state = CONCLUDED.
- Traffic allocation fractions must sum to 1.0 (validated at creation).
- Exactly one variant marked `is_control = true` (validated at creation).
- Every state transition, config change, and guardrail override logged to `audit_trail`.

**Why this first**: M5 is the API surface that every other module and the UI depend on. Without experiment CRUD, nothing else can be configured or started.

## Phase-by-Phase Deliverables

### Phase 0 (Week 1)
- [ ] Go module skeleton with connect-go server
- [ ] PostgreSQL migrations (flyway or golang-migrate)
- [ ] Experiment CRUD stubs

### Phase 1 (Weeks 2–7)
- [ ] Full experiment CRUD with validation
- [ ] State machine enforcement (valid transitions only)
- [ ] STARTING validation: confirm metrics exist, confirm layer has available buckets
- [ ] CONCLUDING trigger: signal M4a to begin final analysis
- [ ] Layer management: create layers, allocate buckets, validate no overlap
- [ ] Bucket reuse: release allocations after cooldown, reject premature reuse
- [ ] Metric definition CRUD
- [ ] Targeting rule CRUD
- [ ] Audit trail on every mutation
- [ ] `StreamConfigUpdates` for M1: server-streaming RPC of experiment config changes
- [ ] Guardrail alert consumer: Kafka consumer on `guardrail_alerts`, auto-pause experiments

### Phase 2 (Weeks 6–11)
- [ ] Interleaving experiment validation: require InterleavingConfig, algorithm_ids ≥ 2
- [ ] Session experiment validation: require SessionConfig, validate session_id_attribute
- [ ] Surrogate model CRUD + calibration trigger
- [ ] Lifecycle stratification power warnings at creation time
- [ ] Auto-conclude sequential experiments when M4a reports boundary crossing

### Phase 3 (Weeks 10–17)
- [ ] Bandit experiment validation: require BanditConfig, validate context feature keys in Redis
- [ ] Cumulative holdout enforcement: prevent auto-conclusion, priority allocation
- [ ] Cold-start bandit auto-creation (from M4b request)
- [ ] Type-specific concluding behavior: policy snapshot for bandits, surrogate projection for all
- [ ] Guardrail override audit: ALERT_ONLY requires explicit owner action with audit log

### Phase 4 (Weeks 16–22)
- [ ] Concurrent experiment stress test: 100 simultaneous state transitions, verify no race conditions
- [ ] Bucket reuse stress test: rapidly create/conclude experiments, verify allocation integrity
- [ ] Audit trail completeness validation: every mutation has a corresponding entry

## Local Development

```bash
# Start PostgreSQL
docker-compose up -d postgres

# Run migrations
golang-migrate -source file://sql/migrations -database postgres://localhost/experimentation up

# Run Go tests
cd services/management
go test -race -cover ./...

# Run server
POSTGRES_DSN=postgres://localhost/experimentation \
KAFKA_BROKERS=localhost:9092 \
go run cmd/main.go

# Create an experiment
grpcurl -plaintext -d '{
  "experiment": {
    "name": "Homepage Hero Test",
    "owner_email": "pm@example.com",
    "type": "AB",
    "layer_id": "layer-default",
    "primary_metric_id": "play_start_rate"
  }
}' localhost:50055 experimentation.management.v1.ExperimentManagementService/CreateExperiment
```

## Testing Expectations

- **Unit tests**: testify for Go. Every state transition path tested (valid + every invalid). Every validation rule has positive and negative tests.
- **Integration**: Docker Compose with PostgreSQL. Full lifecycle test: create → start → run → conclude → archive. Verify audit trail has 5+ entries.
- **Concurrency**: Run 10 goroutines simultaneously trying to start the same experiment. Exactly one succeeds; others get FAILED_PRECONDITION. Verify no partial state.

## Common Pitfalls

1. **State transition atomicity**: Use PostgreSQL `UPDATE experiments SET state = 'RUNNING' WHERE experiment_id = $1 AND state = 'STARTING'` with `RowsAffected() == 1` check. This is your optimistic lock. If another request transitioned the state first, yours fails cleanly.
2. **Bucket reuse timing**: `reusable_after = released_at + cooldown`. Don't compute this in Go — compute it in the SQL query so there's no timezone/clock skew issue.
3. **Guardrail consumer lag**: If your Kafka consumer falls behind, auto-pause will be delayed. Monitor consumer lag. If lag > 5 minutes, alert.
4. **Config stream backpressure**: `StreamConfigUpdates` must handle slow M1 consumers. Use buffered channels in Go. If M1 is too slow, log a warning but don't block other consumers.
5. **Variant fraction validation**: Sum of traffic_fractions must equal 1.0. But floating-point: `0.33 + 0.33 + 0.34 = 1.0000000000000002`. Accept sums within [0.999, 1.001].
