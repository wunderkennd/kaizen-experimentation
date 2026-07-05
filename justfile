# ==============================================================================
# Experimentation Platform — Justfile
# ==============================================================================
# Usage:
#   just setup       — Full local setup (infra + codegen + verify)
#   just test        — Run all tests (Rust + Go + TypeScript + hash parity)
#   just dev         — Start infra + seed data for local development
#   just clean       — Tear down all local infra and generated code
#   just --list      — Show all available recipes
# ==============================================================================

# Load .env file if present
set dotenv-load

# Use bash for all recipes
set shell := ["bash", "-euo", "pipefail", "-c"]

# Default recipe when running `just` with no arguments
default: list

# ------------------------------------------------------------------------------
# Configuration
# ------------------------------------------------------------------------------

buf          := "buf"
cargo        := "cargo"
go           := "go"
npm          := "npm"
k6           := "k6"
docker       := "docker compose"

proto_dir    := "proto"
gen_go_dir   := "gen/go"
gen_ts_dir   := "gen/ts"
services_dir := "services"
crates_dir   := "crates"
ui_dir       := "ui"

# Env vars with defaults (overridable via .env or shell)
pg_host     := env("POSTGRES_HOST", "localhost")
pg_port     := env("POSTGRES_PORT", "5432")
pg_user     := env("POSTGRES_USER", "experimentation")
pg_db       := env("POSTGRES_DB", "experimentation")
pg_password := env("POSTGRES_PASSWORD", "localdev")

# ==============================================================================
# Setup & Development
# ==============================================================================

# Full first-time setup — infra, codegen, install deps, restore agent skills, seed data, verify
setup: infra codegen deps install-skills seed test
    @echo ""
    @echo "============================================"
    @echo "  Setup complete. All tests passing."
    @echo "============================================"

# Start infra + seed data (skip tests for fast iteration)
dev: infra seed
    @echo ""
    @echo "  Infrastructure running. Seed data loaded."
    @echo "  Kafka UI:     http://localhost:8080"
    @echo "  Schema Reg:   http://localhost:8081"
    @echo "  PostgreSQL:   localhost:5432"
    @echo "  Redis:        localhost:6379"
    @echo ""

# Tear down infra, remove generated code and build artifacts
clean: infra-down monitoring-down
    rm -rf {{ gen_go_dir }} {{ gen_ts_dir }}
    rm -rf target
    rm -rf {{ ui_dir }}/node_modules {{ ui_dir }}/.next
    @echo "  Cleaned."

# ==============================================================================
# Infrastructure
# ==============================================================================

# Start local infrastructure (Postgres, Kafka, Redis, Schema Registry)
infra:
    @echo "  Starting infrastructure..."
    {{ docker }} up -d --wait
    @echo "  Infrastructure healthy."

# Stop and remove infrastructure (preserves volumes)
infra-down:
    {{ docker }} down

# Stop infrastructure and destroy all volumes
infra-reset:
    {{ docker }} down -v
    @echo "  Volumes destroyed. Run 'just infra seed' to re-initialize."

# ==============================================================================
# Code Generation (Protobuf)
# ==============================================================================

# Generate Go + TypeScript code from proto schemas
codegen: _codegen-check codegen-go codegen-ts
    @echo "  Code generation complete."

# Generate Go ConnectRPC stubs
codegen-go:
    @echo "  Generating Go stubs..."
    mkdir -p {{ gen_go_dir }}
    cd {{ proto_dir }} && {{ buf }} generate --template buf.gen.yaml --path experimentation

# Generate TypeScript ConnectRPC stubs
codegen-ts:
    @echo "  Generating TypeScript stubs..."
    mkdir -p {{ gen_ts_dir }}
    cd {{ proto_dir }} && {{ buf }} generate --template buf.gen.yaml --path experimentation

# Lint proto schemas and check for breaking changes
lint-proto:
    cd {{ proto_dir }} && {{ buf }} lint
    @echo "  Proto lint passed."

# (internal) Verify buf is installed
_codegen-check:
    @command -v {{ buf }} >/dev/null 2>&1 || { echo "  Error: 'buf' not found. Install: https://buf.build/docs/installation"; exit 1; }

# ==============================================================================
# Dependencies
# ==============================================================================

# Install all project dependencies
deps: deps-rust deps-go deps-ts

# Fetch Rust dependencies and check workspace compiles
deps-rust:
    @echo "  Checking Rust toolchain..."
    {{ cargo }} check --workspace 2>/dev/null || {{ cargo }} fetch

# Download Go module dependencies
deps-go:
    @echo "  Installing Go dependencies..."
    cd {{ services_dir }} && {{ go }} mod download

# Install TypeScript/Node dependencies
deps-ts:
    @echo "  Installing TypeScript dependencies..."
    cd {{ ui_dir }} && {{ npm }} ci --prefer-offline 2>/dev/null || {{ npm }} install

# ==============================================================================
# Agent Skills (Claude Code, Antigravity, Devin, Gemini CLI, Junie, Warp)
# ==============================================================================

# Restore project skills from skills-lock.json (run on fresh clone or after pull)
install-skills:
    @echo "  Restoring agent skills from skills-lock.json..."
    npx --yes skills experimental_install -y

# Check for skill updates available upstream
update-skills-check:
    npx --yes skills check

# Pull latest versions of locked skills (commit the updated skills-lock.json)
update-skills:
    npx --yes skills update -p -y

# Validate the canonical agent registry (docs/agents/registry/) against the
# OKF v0.1 conformance rules. The registry is the single source of truth for
# agent identity — see docs/coordination/harness-modernization-proposal.md §7.
check-registry:
    python3 scripts/check_okf.py docs/agents/registry

# Lint delivery-lifecycle documents (ADR headers, plan v2 sections, PRD
# one-metric rule) — advisory; DOCS_LINT_STRICT=1 escalates warnings.
# See docs/guides/delivery-lifecycle.md (H7, #699).
check-docs:
    python3 scripts/check_docs.py .

# Offline tests for the docs lint
test-check-docs:
    @bash scripts/test_check_docs.sh

# Restore the optional third-party agent library (msitarzewski/agency-agents)
# into .claude/agents/. Not committed (gitignored except repo-authored agents
# like pr-triage.md) — same pattern as skills. Safe to skip; nothing in the
# harness depends on these personas.
install-agents:
    #!/usr/bin/env bash
    set -euo pipefail
    TMP=$(mktemp -d)
    trap 'rm -rf "$TMP"' EXIT
    git clone --depth 1 https://github.com/msitarzewski/agency-agents "$TMP"
    rsync -a --exclude '.git' "$TMP"/ .claude/agents/
    echo "✓ Agency agents restored into .claude/agents/ (gitignored)"

# ==============================================================================
# Testing
# ==============================================================================

# Run all test suites
test: test-rust test-go test-ts test-hash test-infra
    @echo ""
    @echo "  All tests passed."

# Run Rust workspace tests
test-rust:
    @echo "  Running Rust tests..."
    {{ cargo }} test --workspace

# Run Go tests with race detection
test-go:
    @echo "  Running Go tests..."
    cd {{ services_dir }} && {{ go }} test -race -cover ./...

# Run TypeScript tests
test-ts:
    @echo "  Running TypeScript tests..."
    cd {{ ui_dir }} && {{ npm }} test -- --passWithNoTests

# Validate hash parity across implementations (10,000 vectors)
test-hash:
    @echo "  Validating hash parity..."
    python3 scripts/verify_hash_parity.py

# MetricQL cross-implementation parity test (ADR-026 Phase 2 / #436).
# Runs both the Rust and Go parsers against the shared golden corpus at
# test-vectors/metricql_corpus.json and asserts both implementations
# accept/reject the same fixtures with the same extracted refs.
test-metricql-parity:
    @echo "  Rust parser parity..."
    {{ cargo }} test -p experimentation-management --test metricql_corpus_parity -- --nocapture
    @echo "  Go parser parity..."
    cd {{ services_dir }} && {{ go }} test ./metrics/internal/metricql/ -run TestCorpusParity -v
    @echo "  ✓ both parsers accept/reject the same corpus"

# Run Rust flags service tests (ADR-024: Go M7 deleted, Rust crate is the implementation)
test-flags:
    @echo "  Running Rust flags service tests..."
    {{ cargo }} test -p experimentation-flags

# Run ADR-026 Phase 1 validation + contract tests across Rust M5 + Go M3
test-adr026:
    @echo "  Running ADR-026 Phase 1 validation tests..."
    {{ cargo }} test -p experimentation-management metric
    @echo "  Running ADR-026 Phase 1 contract tests..."
    cd {{ services_dir }} && {{ go }} test ./metrics/internal/ -run "TestM3M5|TestM3M4|TestContract_MetricSummaries|TestContract_NewMetricTypes"

# Apply ADR-026 migrations to the local dev Postgres (Phase 1 + Phase 2)
migrate-adr026:
    @echo "  Applying ADR-026 Phase 1 migration (011)..."
    psql $DATABASE_URL -f sql/migrations/011_adr026_phase1_metric_types.sql
    @echo "  Applying ADR-026 Phase 1 #475 migration (012)..."
    psql $DATABASE_URL -f sql/migrations/012_metric_computation_status.sql
    @echo "  Applying ADR-026 Phase 2 #435 migration (013)..."
    psql $DATABASE_URL -f sql/migrations/013_adr026_phase2_metricql_expression.sql

# Run ADR-026 Phase 2 #435 MetricQL parser/compiler tests + M3 integration
test-adr026-phase2:
    @echo "  Running ADR-026 Phase 2 MetricQL package tests..."
    cd {{ services_dir }} && {{ go }} test ./metrics/internal/metricql/... -v
    @echo "  Running ADR-026 Phase 2 M3 integration tests..."
    cd {{ services_dir }} && {{ go }} test ./metrics/internal/jobs/ -run "TestTopologicalOrder_Metricql|TestStandardJob_Run_Metricql" -v

# Run ADR-026 Phase 1 #475 M3 dependency-ordering tests (unit + integration)
test-adr026-m3:
    @echo "  Running ADR-026 #475 M3 scheduler unit tests..."
    cd {{ services_dir }} && {{ go }} test ./metrics/internal/jobs/ -run "TestTopologicalOrder|TestStatusMap|TestStandardJob_Run_Composite|TestStandardJob_Run_FailFast" -v
    @echo "  Running ADR-026 #475 multi-cycle integration test (requires migration 012 + test DB up)..."
    cd {{ services_dir }} && {{ go }} test -tags=integration ./metrics/internal/ -run "TestComputeMetrics_CompositeOrdering" -v

# Run integration tests against local infra
test-integration: infra
    @echo "  Running integration tests..."
    cd {{ services_dir }} && {{ go }} test -race -tags=integration ./...

# Run a specific Rust crate's tests (e.g., just test-crate experimentation-hash)
test-crate crate:
    @echo "  Running tests for {{ crate }}..."
    {{ cargo }} test -p {{ crate }}

# Run bootstrap coverage validation (1000 datasets × 4 scenarios, ~30s in release)
test-bootstrap-coverage:
    @echo "  Running bootstrap coverage validation (release mode)..."
    {{ cargo }} test --release -p experimentation-stats --test bootstrap_coverage -- --ignored --nocapture

# Run infrastructure unit and mock tests (no AWS credentials needed)
test-infra:
    @echo "  Running infra tests..."
    cd infra && {{ go }} test -race -count=1 ./pkg/... ./test/...

# Run infra preview against real AWS (requires AWS credentials)
test-infra-preview:
    @echo "  Running infra preview tests..."
    cd infra && {{ go }} test -race -count=1 -tags=preview -timeout=5m ./test/...

# ==============================================================================
# Linting
# ==============================================================================

# Run all linters
lint: lint-proto lint-rust lint-go lint-ts

# Run Rust clippy with all features
lint-rust:
    {{ cargo }} clippy --workspace --all-features -- -D warnings

# Run Go vet
lint-go:
    cd {{ services_dir }} && {{ go }} vet ./...

# Run TypeScript/ESLint
lint-ts:
    cd {{ ui_dir }} && {{ npm }} run lint

# Format all Rust code
fmt:
    {{ cargo }} fmt --all

# Check Rust formatting without modifying
fmt-check:
    {{ cargo }} fmt --all -- --check

# ==============================================================================
# Benchmarks
# ==============================================================================

# Run Rust benchmarks (hash + stats)
bench:
    @echo "  Running benchmarks..."
    {{ cargo }} bench --workspace

# Run benchmarks for a specific crate (e.g., just bench-crate experimentation-hash)
bench-crate crate:
    @echo "  Running benchmarks for {{ crate }}..."
    {{ cargo }} bench -p {{ crate }}

# Build assignment service with PGO optimization (instrument → profile → optimize)
pgo-build:
    @echo "  Building PGO-optimized assignment service..."
    bash scripts/pgo_build.sh

# Build policy service with PGO optimization (instrument → profile → optimize)
pgo-build-policy:
    @echo "  Building PGO-optimized policy service..."
    bash scripts/pgo_build_policy.sh

# Build analysis service with PGO optimization (instrument → profile → optimize)
pgo-build-analysis:
    @echo "  Building PGO-optimized analysis service..."
    bash scripts/pgo_build_analysis.sh

# Build assignment service release binary (no PGO)
build-assignment-release:
    {{ cargo }} build --release --package experimentation-assignment

# ==============================================================================
# Seed Data
# ==============================================================================

# Load development seed data into local Postgres
seed:
    @echo "  Loading seed data..."
    PGPASSWORD={{ pg_password }} psql \
        -h {{ pg_host }} \
        -p {{ pg_port }} \
        -U {{ pg_user }} \
        -d {{ pg_db }} \
        -f scripts/seed_dev.sql \
        --quiet
    @echo "  Seed data loaded."

# Open a psql shell to the local database
db:
    PGPASSWORD={{ pg_password }} psql \
        -h {{ pg_host }} \
        -p {{ pg_port }} \
        -U {{ pg_user }} \
        -d {{ pg_db }}

# ==============================================================================
# Docker Build
# ==============================================================================

# Build all service Docker images
docker-build:
    @echo "  Building Docker images..."
    # Rust services
    for svc in assignment analysis pipeline policy; do \
        echo "  Building experimentation-$svc..."; \
        docker build -t experimentation-$svc -f {{ crates_dir }}/experimentation-$svc/Dockerfile .; \
    done
    # Go services
    for svc in management metrics flags orchestration; do \
        echo "  Building experimentation-$svc..."; \
        docker build -t experimentation-$svc -f {{ services_dir }}/$svc/Dockerfile .; \
    done
    # UI
    docker build -t experimentation-ui -f {{ ui_dir }}/Dockerfile .
    @echo "  All images built."

# Build a single service image (e.g., just docker-build-svc assignment)
docker-build-svc svc:
    @echo "  Building experimentation-{{ svc }}..."
    @if [ -f "{{ crates_dir }}/experimentation-{{ svc }}/Dockerfile" ]; then \
        docker build -t experimentation-{{ svc }} -f {{ crates_dir }}/experimentation-{{ svc }}/Dockerfile .; \
    elif [ -f "{{ services_dir }}/{{ svc }}/Dockerfile" ]; then \
        docker build -t experimentation-{{ svc }} -f {{ services_dir }}/{{ svc }}/Dockerfile .; \
    elif [ -f "{{ ui_dir }}/Dockerfile" ] && [ "{{ svc }}" = "ui" ]; then \
        docker build -t experimentation-ui -f {{ ui_dir }}/Dockerfile .; \
    else \
        echo "  Error: No Dockerfile found for {{ svc }}"; exit 1; \
    fi

# ==============================================================================
# Monitoring Stack
# ==============================================================================

# Start Grafana + Prometheus + Jaeger alongside main infra
monitoring:
    @echo "  Starting monitoring stack..."
    {{ docker }} -f docker-compose.yml -f docker-compose.monitoring.yml up -d --wait
    @echo "  Grafana:      http://localhost:3000  (admin/admin)"
    @echo "  Prometheus:   http://localhost:9090"
    @echo "  Jaeger:       http://localhost:16686"

# Stop monitoring stack
monitoring-down:
    -{{ docker }} -f docker-compose.yml -f docker-compose.monitoring.yml down 2>/dev/null

# ==============================================================================
# Load Testing
# ==============================================================================

# Run k6 load test against local services (steady-state)
loadtest:
    @command -v {{ k6 }} >/dev/null 2>&1 || { echo "  Error: 'k6' not found. Install: https://k6.io/docs/get-started/installation/"; exit 1; }
    {{ k6 }} run scripts/loadtest.js

# Run k6 spike test
loadtest-spike:
    @command -v {{ k6 }} >/dev/null 2>&1 || { echo "  Error: 'k6' not found."; exit 1; }
    {{ k6 }} run scripts/loadtest.js --env SCENARIO=spike

# Run k6 soak test (30 minutes)
loadtest-soak:
    @command -v {{ k6 }} >/dev/null 2>&1 || { echo "  Error: 'k6' not found."; exit 1; }
    {{ k6 }} run scripts/loadtest.js --env SCENARIO=soak

# Run M1 assignment service load test: p99 < 5ms at 10K rps (builds, starts server, validates SLA)
loadtest-assignment:
    bash scripts/loadtest_assignment.sh

# Run M1 assignment service load test at 50K rps (Phase 4 SLA validation)
loadtest-assignment-50k:
    TARGET_RPS=50000 DURATION=60s bash scripts/loadtest_assignment.sh


# Run M7 flag service load test: p99 < 10ms at 20K rps (builds, starts server, seeds flags, validates SLA)
loadtest-flags:
    bash scripts/loadtest_flags.sh

# Run M4b policy service load test: p99 < 15ms at 10K rps (builds, starts server, seeds experiments, validates SLA)
loadtest-policy:
    bash scripts/loadtest_policy.sh

# Run M2 pipeline load test: p99 < 10ms at 10K rps (builds, starts server, validates SLA)
loadtest-pipeline:
    bash scripts/loadtest_pipeline.sh

# Build pipeline service with PGO optimization (instrument → profile → optimize)
pgo-build-pipeline:
    @echo "  Building PGO-optimized pipeline service..."
    bash scripts/pgo_build_pipeline.sh

# ==============================================================================
# Chaos Engineering
# ==============================================================================

# Run all chaos tests (M1 assignment + M4b policy + M2 pipeline + verify)
chaos: chaos-assignment chaos-policy chaos-analysis chaos-pipeline chaos-verify
    @echo ""
    @echo "  All chaos tests passed."

# Run M1 assignment kill-9 chaos test (stateless, recovery < 2s)
chaos-assignment:
    bash scripts/chaos_kill_assignment.sh

# Run M4b policy kill-9 chaos test (RocksDB recovery < 10s)
chaos-policy:
    bash scripts/chaos_kill_policy.sh

# Run M2 pipeline kill-9 chaos test (Kafka idempotent producer, no data loss)
chaos-pipeline:
    bash scripts/chaos_kill_ingestion.sh

# Run M4a analysis kill-9 chaos test (stateless, recovery < 2s)
chaos-analysis:
    bash scripts/chaos_test_analysis.sh

# Verify Kafka data integrity after chaos tests
chaos-verify:
    bash scripts/chaos_verify_integrity.sh

# Run the multi-service chaos E2E framework (requires Docker infra)
chaos-framework:
    bash scripts/chaos_e2e_framework.sh

# Run chaos framework for specific services
chaos-framework-services services:
    bash scripts/chaos_e2e_framework.sh --services {{ services }}

# ==============================================================================
# Local Services
# ==============================================================================
#
# Run individual services or the full platform locally.
# Prerequisites: `just dev` (starts Postgres, Kafka, Redis, Schema Registry).
#
# Startup order (respects dependency graph):
#   Tier 0: M5 (Management) — owns PG schema, config hub
#   Tier 1: M1, M2, M4b, M7 — depend on M5 and/or Kafka
#   Tier 2: M3, M4a — depend on Tier 1 services
#   Tier 3: M6 (UI) — connects to M5
#

# Common env vars for local development
db_url       := env("DATABASE_URL", "postgresql://experimentation:localdev@localhost:5432/experimentation?sslmode=disable")
kafka_broker := env("KAFKA_BROKERS", "localhost:9092")

# --- Individual service recipes ---

# Run M5 Management service (Go — config hub, CRUD, lifecycle)
run-m5:
    @echo "  Starting M5 Management (port 50055)..."
    cd {{ services_dir }} && \
        PORT=50055 \
        METRICS_PORT=50060 \
        DATABASE_URL={{ db_url }} \
        KAFKA_BROKERS={{ kafka_broker }} \
        DISABLE_AUTH=true \
        {{ go }} run ./management/cmd/

# Run M1 Assignment service (Rust — variant allocation)
run-m1:
    @echo "  Starting M1 Assignment (port 50051)..."
    GRPC_ADDR=0.0.0.0:50051 \
    HTTP_ADDR=0.0.0.0:8080 \
    M5_ADDR=http://localhost:50055 \
    {{ cargo }} run --package experimentation-assignment

# Run M2 Pipeline service (Rust — event ingestion + Kafka)
run-m2:
    @echo "  Starting M2 Pipeline (port 50052)..."
    PORT=50052 \
    METRICS_PORT=9091 \
    KAFKA_BROKERS={{ kafka_broker }} \
    BUFFER_DIR=/tmp/experimentation-pipeline-buffer \
    {{ cargo }} run --package experimentation-pipeline

# Run M3 Metrics service (Go — Spark SQL orchestration)
run-m3:
    @echo "  Starting M3 Metrics (port 50056)..."
    cd {{ services_dir }} && \
        PORT=50056 \
        METRICS_PORT=50059 \
        POSTGRES_URL={{ db_url }} \
        KAFKA_BROKERS={{ kafka_broker }} \
        {{ go }} run ./metrics/cmd/

# Run M4a Analysis service (Rust — statistical computation)
run-m4a:
    @echo "  Starting M4a Analysis (port 50053)..."
    ANALYSIS_GRPC_ADDR=[::1]:50053 \
    DELTA_LAKE_PATH=/tmp/delta \
    DATABASE_URL={{ db_url }} \
    {{ cargo }} run --package experimentation-analysis

# Run M4b Policy service (Rust — bandit engine, LMAX core)
run-m4b:
    @echo "  Starting M4b Policy (port 50054)..."
    POLICY_GRPC_ADDR=[::1]:50054 \
    POLICY_ROCKSDB_PATH=/tmp/experimentation-policy-rocksdb \
    KAFKA_BROKERS={{ kafka_broker }} \
    KAFKA_GROUP_ID=bandit-policy-service \
    KAFKA_REWARD_TOPIC=reward_events \
    {{ cargo }} run --package experimentation-policy

# Run M6 UI (TypeScript/Next.js — web dashboard)
run-m6:
    @echo "  Starting M6 UI (port 3000)..."
    cd {{ ui_dir }} && {{ npm }} run dev

# Run M7 Flags service (Rust — feature flags + percentage rollout)
run-m7:
    @echo "  Starting M7 Flags (port 50057)..."
    DATABASE_URL={{ db_url }} \
    FLAGS_GRPC_ADDR=[::]:50057 \
    FLAGS_ADMIN_ADDR=[::]:9090 \
    M5_ADDR=http://localhost:50055 \
    KAFKA_BROKERS={{ kafka_broker }} \
    FLAGS_KAFKA_ENABLED=false \
    {{ cargo }} run --package experimentation-flags

# Run M2-Orch Orchestration service (Go — metrics pipeline orchestration)
run-m2-orch:
    @echo "  Starting M2-Orch Orchestration (port 50058)..."
    cd {{ services_dir }} && \
        PORT=50058 \
        POSTGRES_URL={{ db_url }} \
        {{ go }} run ./orchestration/cmd/

# --- Combined recipe ---

# Run all services locally (requires `just dev` for infra)
run-all: _check-infra
    #!/usr/bin/env bash
    set -euo pipefail
    trap 'echo "  Stopping all services..."; kill $(jobs -p) 2>/dev/null; wait' EXIT INT TERM

    echo "============================================"
    echo "  Starting all Kaizen services"
    echo "============================================"
    echo ""

    # Tier 0: M5 Management (config hub, must be first)
    echo "  [Tier 0] M5 Management..."
    cd services && \
        PORT=50055 METRICS_PORT=50060 \
        DATABASE_URL="{{ db_url }}" \
        KAFKA_BROKERS="{{ kafka_broker }}" \
        DISABLE_AUTH=true \
        go run ./management/cmd/ &
    sleep 3

    # Tier 1: Core services (depend on M5 / Kafka)
    echo "  [Tier 1] M1 Assignment, M2 Pipeline, M4b Policy, M7 Flags..."
    GRPC_ADDR=0.0.0.0:50051 HTTP_ADDR=0.0.0.0:8080 \
        M5_ADDR=http://localhost:50055 \
        cargo run --package experimentation-assignment &

    PORT=50052 METRICS_PORT=9091 \
        KAFKA_BROKERS="{{ kafka_broker }}" \
        BUFFER_DIR=/tmp/experimentation-pipeline-buffer \
        cargo run --package experimentation-pipeline &

    POLICY_GRPC_ADDR=[::1]:50054 \
        POLICY_ROCKSDB_PATH=/tmp/experimentation-policy-rocksdb \
        KAFKA_BROKERS="{{ kafka_broker }}" \
        KAFKA_GROUP_ID=bandit-policy-service \
        KAFKA_REWARD_TOPIC=reward_events \
        cargo run --package experimentation-policy &

    DATABASE_URL="{{ db_url }}" \
        FLAGS_GRPC_ADDR=[::]:50057 FLAGS_ADMIN_ADDR=[::]:9090 \
        M5_ADDR=http://localhost:50055 \
        KAFKA_BROKERS="{{ kafka_broker }}" FLAGS_KAFKA_ENABLED=false \
        cargo run --package experimentation-flags &
    sleep 3

    # Tier 2: Dependent services
    echo "  [Tier 2] M3 Metrics, M4a Analysis, M2-Orch..."
    cd services && \
        PORT=50056 METRICS_PORT=50059 \
        POSTGRES_URL="{{ db_url }}" \
        KAFKA_BROKERS="{{ kafka_broker }}" \
        go run ./metrics/cmd/ &

    ANALYSIS_GRPC_ADDR=[::1]:50053 \
        DELTA_LAKE_PATH=/tmp/delta \
        DATABASE_URL="{{ db_url }}" \
        cargo run --package experimentation-analysis &

    cd services && \
        PORT=50058 POSTGRES_URL="{{ db_url }}" \
        go run ./orchestration/cmd/ &
    sleep 2

    # Tier 3: UI
    echo "  [Tier 3] M6 UI..."
    cd ui && npm run dev &

    echo ""
    echo "============================================"
    echo "  All services starting. Ports:"
    echo "    M1  Assignment:   localhost:50051 (gRPC) / localhost:8080 (HTTP)"
    echo "    M2  Pipeline:     localhost:50052"
    echo "    M2  Orchestration:localhost:50058"
    echo "    M3  Metrics:      localhost:50056"
    echo "    M4a Analysis:     localhost:50053"
    echo "    M4b Policy:       localhost:50054"
    echo "    M5  Management:   localhost:50055"
    echo "    M6  UI:           localhost:3000"
    echo "    M7  Flags:        localhost:50057"
    echo "============================================"
    echo "  Press Ctrl+C to stop all services."
    echo ""
    wait

# (internal) Verify infra is running before starting services
_check-infra:
    @{{ docker }} ps 2>/dev/null | grep -q postgres \
        || { echo "  Error: Local infra not running. Run 'just dev' first."; exit 1; }

# ==============================================================================
# Convenience
# ==============================================================================

# Show all available recipes
list:
    @just --list --unsorted

# Print current status of local infrastructure
status:
    @echo "  Docker services:"
    @{{ docker }} ps --format "table {{{{.Name}}}}\t{{{{.Status}}}}\t{{{{.Ports}}}}" 2>/dev/null || echo "    (not running)"
    @echo ""
    @echo "  Postgres:"
    @PGPASSWORD={{ pg_password }} psql -h {{ pg_host }} -p {{ pg_port }} -U {{ pg_user }} -d {{ pg_db }} \
        -c "SELECT state, COUNT(*) FROM experiments GROUP BY state" --quiet 2>/dev/null \
        || echo "    (not reachable)"
    @echo ""
    @echo "  Kafka topics:"
    @docker exec -i $$({{ docker }} ps -q --filter name=redpanda 2>/dev/null | head -1) \
        rpk topic list --brokers=localhost:9092 2>/dev/null \
        || echo "    (not reachable)"

# Watch Rust workspace for changes and re-run tests
watch:
    {{ cargo }} watch -x "test --workspace"

# Run a single experiment through the full local pipeline (smoke test)
smoke-test: infra seed
    @echo "  Running smoke test..."
    @echo "  ✓ Infrastructure up"
    @PGPASSWORD={{ pg_password }} psql -h {{ pg_host }} -p {{ pg_port }} -U {{ pg_user }} -d {{ pg_db }} \
        -c "SELECT experiment_id, name, state FROM experiments LIMIT 5" --quiet
    @echo "  ✓ Seed data present"
    @echo "  Smoke test passed."

# Dispatch a test coverage task to Jules
jules-tests crate:
    #!/usr/bin/env bash
    set -euo pipefail
    REPO=$(gh repo view --json nameWithOwner -q .nameWithOwner)
    jules remote new --repo "$REPO" \
      --session "Write unit tests for crates/{{crate}}/. Target 80% coverage. Tests only."

# Dispatch a golden-file task to Devin
devin-golden-files:
    @echo "Submit via Devin web UI or Slack with this prompt:"
    @echo "Generate golden-file tests for crates/experimentation-stats/tests/"
    @echo "using reference outputs from R packages. See docs/adrs/ for target precision."

# Quick second opinion via Gemini
gemini-review file:
    gemini -p "Review this Rust implementation for correctness, edge cases, and potential panics: $(cat {{file}})"

# Triage open PRs (invokes pr-triage subagent)
pr-triage:
    claude -p "Use the pr-triage agent. There are open PRs that need triage after a system restart. Inventory all open PRs, categorize them, present the summary, and wait for my confirmation before acting."

# ============================================
# Phase 5: Agent Orchestration
# ============================================
#
# Three modes:
#   just interactive    — Gas Town (you're steering, Mayor + polecats)
#   just autonomous     — Multiclaude (you're away, daemon + merge queue)
#   just solo           — Single Claude Code session (quick one-off task)
#
# Work tracked via GitHub Issues (not status files):
#   just sprint-status  — Current sprint Issues
#   just blocked        — Blocked Issues
#   just work-on 42     — Launch a worker from Issue #42
#

# --- Configuration ---

kaizen_repo  := env("GT_HOME", "~/gt")
mc_session   := "mc-kaizen"

# --- Morning Handoff (Multiclaude → Gas Town) ---

morning:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "=== Morning Handoff ==="
    echo ""
    echo "--- Multiclaude overnight results ---"
    multiclaude status 2>/dev/null || echo "(Multiclaude not running)"
    echo ""
    echo "--- Open PRs ---"
    gh pr list --limit 20 2>/dev/null || echo "(gh not configured)"
    echo ""
    echo "--- Sprint Status (GitHub Issues) ---"
    MILESTONE=$(gh api repos/:owner/:repo/milestones --jq '[.[] | select(.state=="open")] | sort_by(.due_on) | .[0].title' 2>/dev/null || echo "")
    if [ -n "$MILESTONE" ]; then
      echo "Active milestone: $MILESTONE"
      echo ""
      echo "  Open:"
      gh issue list --milestone "$MILESTONE" --state open --json number,title,assignees,labels \
        --jq '.[] | "    #\(.number) [\(.assignees | map(.login) | join(",") // "unassigned")] \(.title)"' 2>/dev/null
      echo ""
      echo "  Closed (recently):"
      gh issue list --milestone "$MILESTONE" --state closed --limit 5 --json number,title \
        --jq '.[] | "    #\(.number) ✓ \(.title)"' 2>/dev/null
      echo ""
      echo "  Blocked:"
      gh issue list --label "blocked" --state open --json number,title \
        --jq '.[] | "    #\(.number) ⚠ \(.title)"' 2>/dev/null
    else
      echo "(No active milestone found)"
    fi
    echo ""
    echo "--- Pulling latest main ---"
    git checkout main 2>/dev/null && git pull origin main 2>/dev/null
    echo ""
    if bd list --all --json >/dev/null 2>&1; then
      echo "--- Close-syncing beads (mirroring GH Issue closures) ---"
      just beads-close-sync 2>/dev/null || true
      echo ""
    fi
    echo "--- Sweeping stale worker claims (H1 lease expiry, #679) ---"
    bash scripts/orchestration/claims.sh sweep 2>/dev/null || true
    echo ""
    echo "Next steps:"
    echo "  just autonomous-stop     # stop overnight workers"
    echo "  just interactive         # start Gas Town for the day"

# --- Evening Handoff (Gas Town → Multiclaude) ---

evening sprint_num:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "=== Evening Handoff ==="
    (cd "$(eval echo {{kaizen_repo}})" && gt down 2>/dev/null) || true
    git checkout main && git pull origin main
    if bd list --all --json >/dev/null 2>&1; then
      just beads-sync {{sprint_num}} || echo "⚠ beads-sync failed; continuing with autonomous-sprint"
    fi
    just autonomous-sprint {{sprint_num}}
    echo ""
    echo "Workers running. Detach with Ctrl-b d. Check tomorrow with 'just morning'."

# --- Work Tracking (GitHub Issues) ---

# Show current sprint Issues (queries by label, falls back to milestone)
sprint-status sprint_label="":
    #!/usr/bin/env bash
    if [ -z "{{sprint_label}}" ]; then
      # Find the earliest open sprint label with issues
      for s in 5.0 5.1 5.2 5.3 5.4 5.5; do
        COUNT=$(gh issue list --label "sprint-$s" --state open --json number --jq 'length' 2>/dev/null)
        if [ "$COUNT" -gt 0 ]; then
          LABEL="sprint-$s"
          break
        fi
      done
      LABEL="${LABEL:-sprint-5.0}"
    else
      LABEL="sprint-{{sprint_label}}"
    fi
    echo "=== $LABEL ==="
    gh issue list --label "$LABEL" \
      --json number,title,state,assignees,labels \
      --jq '.[] | "\(.state)\t\(.assignees | map(.login) | join(",") // "unassigned")\t#\(.number)\t\(.title)"'

# Show blocked Issues
blocked:
    gh issue list --label "blocked" --state open --json number,title,assignees \
      --jq '.[] | "#\(.number)\t\(.assignees | map(.login) | join(",") // "unassigned")\t\(.title)"'

# Show a specific agent's open work
agent-work agent_label:
    gh issue list --label "{{agent_label}}" --state open --json number,title,milestone \
      --jq '.[] | "#\(.number)\t\(.milestone.title // "no milestone")\t\(.title)"'

# --- GitHub Projects & Goals (see docs/guides/projects-and-goals.md) ---
# These are ADDITIVE. The Milestone-based recipes above keep working until a
# full transition sprint has run on both systems (per the migration plan).

# Create the kaizen Project (v2) + fields. Dry-run unless `apply=1`.
project-bootstrap apply="0":
    #!/usr/bin/env bash
    OWNER=$(gh repo view --json owner --jq '.owner.login')
    FLAG=$([ "{{apply}}" = "1" ] && echo "--apply" || echo "")
    ./scripts/projects/bootstrap-project.sh --owner "$OWNER" $FLAG

# Migrate open Milestones → Iteration field + sprint-N labels. Dry-run unless `apply=1`.
project-migrate project apply="0":
    #!/usr/bin/env bash
    OWNER=$(gh repo view --json owner --jq '.owner.login')
    FLAG=$([ "{{apply}}" = "1" ] && echo "--apply" || echo "")
    ./scripts/projects/migrate-milestones-to-iterations.sh --owner "$OWNER" --project "{{project}}" $FLAG

# List open Goal issues (label:goal) with assignee.
goals:
    gh issue list --label "goal" --state open --json number,title,assignees \
      --jq '.[] | "🎯 #\(.number)\t\(.assignees | map(.login) | join(",") // "unassigned")\t\(.title)"'

# Idempotent: re-running rewrites the <!-- EXEC-BANNER:START/END --> block
# in place without appending. Looks for a plan file under
# docs/superpowers/plans/ that references this issue ("Refs #N", "Closes #N",
# or "Issue: #N"); fails with a clear error if none found.
#
# Run this AFTER the plan PR merges to main; the dispatch recipe `work-on`
# reads only title+body (not comments), so the banner is how locked-plan
# context reaches the Multiclaude worker.
#
# Upsert the locked-plan execution banner on a GitHub Issue.
prime-issue issue:
    #!/usr/bin/env bash
    set -euo pipefail
    ISSUE={{issue}}

    PLAN=$(grep -rlE "(Refs|Closes|Issue:?)[[:space:]]*#${ISSUE}\b" docs/superpowers/plans/*.md 2>/dev/null | head -1)
    if [ -z "$PLAN" ]; then
        echo "✗ No plan in docs/superpowers/plans/ references #${ISSUE}" >&2
        echo "  Plan must contain 'Refs #${ISSUE}', 'Closes #${ISSUE}', or 'Issue: #${ISSUE}'" >&2
        exit 1
    fi

    PLAN_SHA=$(git log -1 --format=%h main -- "$PLAN" 2>/dev/null || true)
    PLAN_SHA=${PLAN_SHA:-(unmerged)}
    PLAN_DATE=$(git log -1 --format=%cs main -- "$PLAN" 2>/dev/null || true)
    PLAN_DATE=${PLAN_DATE:-$(date +%Y-%m-%d)}


    BANNER=$(sed -e "s|\${ISSUE_NUM}|${ISSUE}|g" \
                 -e "s|\${PLAN_PATH}|${PLAN}|g" \
                 -e "s|\${PLAN_SHA}|${PLAN_SHA}|g" \
                 -e "s|\${PLAN_DATE}|${PLAN_DATE}|g" \
                 .github/issue-banner-template.md)

    TMPDIR=$(mktemp -d)
    trap 'rm -rf "$TMPDIR"' EXIT

    gh issue view ${ISSUE} --json body --jq '.body' > "${TMPDIR}/current.md"

    # Strip any prior <!-- EXEC-BANNER --> block plus an optional trailing
    # "---" separator. Python one-liner because just's recipe-body parser
    # requires consistent indentation on every line (multi-line embedded
    # scripts that start at column 0 trip it up).
    python3 -c 'import re,sys; t=open(sys.argv[1]).read(); t=re.sub(r"<!-- EXEC-BANNER:START -->[\s\S]*?<!-- EXEC-BANNER:END -->\n*(---\n*)?", "", t); sys.stdout.write(t.lstrip())' "${TMPDIR}/current.md" > "${TMPDIR}/stripped.md"

    {
      echo "<!-- EXEC-BANNER:START -->"
      echo "$BANNER"
      echo "<!-- EXEC-BANNER:END -->"
      echo ""
      echo "---"
      echo ""
      cat "${TMPDIR}/stripped.md"
    } > "${TMPDIR}/new-body.md"

    gh issue edit ${ISSUE} --body-file "${TMPDIR}/new-body.md"
    echo "✓ Issue #${ISSUE} primed with ${PLAN} (${PLAN_SHA}, ${PLAN_DATE})"

# Same logic as the advisory CI check at .github/workflows/branch-naming.yml;
# exits 1 with suggested renames on no match. Run before `git push -u` to
# catch violations early.
#
# Validate the current branch name against .github/branch-naming.yml.
check-branch-name:
    python3 scripts/check_branch_name.py

# Launch a Multiclaude worker from a GitHub Issue number
work-on issue executor="multiclaude":
    @bash scripts/orchestration/dispatch.sh "{{issue}}" "{{executor}}"

# Dispatch every ready issue for a raw sprint label via a chosen executor
# (H1 generic façade; `autonomous-sprint` stays the sprint-number front door).
sprint label executor="multiclaude":
    #!/usr/bin/env bash
    set -euo pipefail
    COUNT=0; SKIPPED=0
    while IFS= read -r line; do
      num=$(echo "$line" | jq -r '.number')
      echo "  → Issue #$num: $(echo "$line" | jq -r '.title')"
      rc=0
      bash scripts/orchestration/dispatch.sh "$num" "{{executor}}" || rc=$?
      case "$rc" in
        0) COUNT=$((COUNT + 1)) ;;
        3) SKIPPED=$((SKIPPED + 1)); echo "    (already claimed — skipped)" ;;
        *) echo "    ✗ dispatch failed for #$num (rc=$rc)" ;;
      esac
    done < <(bash scripts/orchestration/ready.sh "{{label}}")
    echo "✓ {{label}} via {{executor}}: $COUNT dispatched, $SKIPPED already claimed"

# Offline tests for the dispatch layer (stubbed gh; no network)
test-orchestration:
    @bash scripts/orchestration/test_dispatch.sh

# --- Ecosystem governance (H6) ---
# Sibling-repo onboarding files (caller workflows + ruleset JSON) generated
# from the fleet source of truth: infra/github-governance/Pulumi.governance.yaml.
# Runbook: docs/runbooks/ecosystem-governance.md.

# Generate onboarding files for every fleet repo into dist/governance-onboarding/
governance-onboard outdir="dist/governance-onboarding":
    @python3 scripts/generate_governance_onboarding.py --out {{outdir}}

# Generate AND copy into sibling checkouts living next to this repo (../<repo>)
governance-onboard-apply parent="..":
    @python3 scripts/generate_governance_onboarding.py --out dist/governance-onboarding --apply {{parent}}

# Offline tests for the onboarding generator
test-governance-gen:
    @bash scripts/test_generate_governance_onboarding.sh

# --- Beads (GitHub Issues ↔ Gas Town projection) ---
# GitHub Issues remain the source of truth. Beads are a read-side projection
# into Gas Town so `gt sling`, `gt convoy`, and `gt ready` have work to see.
# Forward sync is explicit (pre-dispatch); close-sync runs in `just morning`.

# One-time setup: initialize .beads/ with kz- prefix
beads-init:
    #!/usr/bin/env bash
    set -euo pipefail
    if [ -d .beads ]; then
      echo "✓ .beads/ already initialized"
      exit 0
    fi
    if ! command -v bd >/dev/null 2>&1; then
      echo "✗ bd not installed. Run: go install github.com/steveyegge/beads/cmd/bd@latest" >&2
      exit 1
    fi
    echo "=== Initializing beads tracker (prefix: kz) ==="
    bd init --prefix kz --skip-agents --skip-hooks
    echo ""
    echo "Next:"
    echo "  just beads-sync 5.1    # materialize open Sprint 5.1 Issues as beads"
    echo "  just beads-sync 5.6    # materialize open Sprint 5.6 Issues as beads"

# Forward-sync: materialize open GH Issues in a sprint as beads (idempotent)
beads-sync sprint_num:
    #!/usr/bin/env bash
    set -euo pipefail
    case "{{sprint_num}}" in
      0|5.0) LABEL="sprint-5.0" ;;
      1|5.1) LABEL="sprint-5.1" ;;
      2|5.2) LABEL="sprint-5.2" ;;
      3|5.3) LABEL="sprint-5.3" ;;
      4|5.4) LABEL="sprint-5.4" ;;
      5|5.5) LABEL="sprint-5.5" ;;
      6|5.6) LABEL="sprint-5.6" ;;
      I.0)   LABEL="sprint-I.0" ;;
      I.1)   LABEL="sprint-I.1" ;;
      I.2)   LABEL="sprint-I.2" ;;
      I.3)   LABEL="sprint-I.3" ;;
      all)   LABEL="--all" ;;
      *) echo "Unknown sprint: {{sprint_num}} (use 0-6, 5.0-5.6, I.0-I.3, or 'all')"; exit 1 ;;
    esac
    bash scripts/beads-sync.sh "$LABEL"

# Close-sync: close beads whose linked GH Issue has been closed
beads-close-sync:
    @bash scripts/beads-close-sync.sh

# Show mirrored beads with current status
beads-status:
    #!/usr/bin/env bash
    if ! bd list --all --json >/dev/null 2>&1; then
      echo "(beads not initialized — run 'just beads-init')"
      exit 0
    fi
    echo "status	bead_id	gh_ref	title"
    bd list --all --json 2>/dev/null \
      | jq -r '.[]
          | select(.external_ref != null)
          | select(.external_ref | startswith("gh-"))
          | "\(.status)\t\(.id)\t\(.external_ref)\t\(.title)"' \
      | sort

# Export beads to git-tracked .beads/issues.jsonl for team sharing
beads-export:
    #!/usr/bin/env bash
    set -euo pipefail
    GIT_COMMON=$(git rev-parse --git-common-dir)
    BEADS_ROOT="$(cd "$(dirname "$GIT_COMMON")" && pwd)/.beads"
    bd export -o "$BEADS_ROOT/issues.jsonl"
    echo "✓ Exported to $BEADS_ROOT/issues.jsonl — commit to share with the team"

# Install a local post-merge git hook that runs beads-close-sync after git pull
# (Local only — not shared via git. Run this once per worktree if you want
# automatic close-sync on every pull. Otherwise `just morning` covers it.)
beads-hooks-install:
    #!/usr/bin/env bash
    set -euo pipefail
    HOOK_DIR="$(git rev-parse --git-path hooks)"
    HOOK="$HOOK_DIR/post-merge"
    MARKER="# BEGIN beads-close-sync"
    if [ -f "$HOOK" ] && grep -qF "$MARKER" "$HOOK"; then
      echo "✓ post-merge hook already installed"
      exit 0
    fi
    mkdir -p "$HOOK_DIR"
    touch "$HOOK"
    chmod +x "$HOOK"
    REPO_ROOT=$(git rev-parse --show-toplevel)
    # Add shebang if file is empty (brand-new hook)
    if [ ! -s "$HOOK" ]; then
      printf '#!/bin/sh\n' > "$HOOK"
    fi
    {
      printf '%s\n' "# Auto-close beads whose linked GH Issue has closed on GitHub"
      printf '%s\n' "if [ -x \"$REPO_ROOT/scripts/beads-close-sync.sh\" ]; then"
      printf '%s\n' "  \"$REPO_ROOT/scripts/beads-close-sync.sh\" >/dev/null 2>&1 || true"
      printf '%s\n' "fi"
      printf '%s\n' "# END beads-close-sync"
    } >> "$HOOK"
    echo "✓ Installed post-merge hook at $HOOK"
    echo "  It runs scripts/beads-close-sync.sh silently after every git pull."

# --- Interactive Mode (Gas Town) ---

interactive:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "=== Interactive Mode (Gas Town) ==="
    cd "$(eval echo {{kaizen_repo}})"
    gt up
    gt mayor attach

interactive-stop:
    cd "$(eval echo {{kaizen_repo}})" && gt down

# --- Autonomous Mode (Multiclaude) ---

autonomous:
    #!/usr/bin/env bash
    multiclaude start 2>/dev/null || true
    echo "Daemon running. Commands:"
    echo "  just work-on <issue-number>    # Launch worker from Issue"
    echo "  just autonomous-sprint <N>     # Launch all sprint workers"
    echo "  just autonomous-status"
    echo "  just autonomous-attach"

autonomous-status:
    multiclaude status

autonomous-attach:
    tmux attach -t {{mc_session}} 2>/dev/null || echo "No active session."

autonomous-stop:
    multiclaude worker kill --all 2>/dev/null || true
    echo "✓ Workers stopped."

autonomous-shutdown:
    multiclaude worker kill --all 2>/dev/null || true
    multiclaude stop 2>/dev/null || true
    echo "✓ Multiclaude fully stopped."

# Internal: emit one JSON object per line for "ready" issues with the given label.
# An issue is ready when (1) it has no open PR closing it, AND (2) every "#N"
# listed under "## Blocked by" in its body refers to a CLOSED issue (or no
# blockers exist). Prefers beads ('bd ready') when initialized.
_ready label:
    @bash scripts/orchestration/ready.sh "{{label}}"

# Sprint launchers read Issues by label (primary) with milestone fallback
autonomous-sprint sprint_num:
    #!/usr/bin/env bash
    set -euo pipefail
    multiclaude start 2>/dev/null || true
    # Normalize sprint number to label format (e.g., "2" → "sprint-5.2", "5.2" → "sprint-5.2")
    case "{{sprint_num}}" in
      0|5.0) LABEL="sprint-5.0"; MS="Sprint 5.0: Schema & Foundations" ;;
      1|5.1) LABEL="sprint-5.1"; MS="Sprint 5.1: Measurement Foundations" ;;
      2|5.2) LABEL="sprint-5.2"; MS="Sprint 5.2: Statistical Core" ;;
      3|5.3) LABEL="sprint-5.3"; MS="Sprint 5.3: Constraints & New Experiment Types" ;;
      4|5.4) LABEL="sprint-5.4"; MS="Sprint 5.4: Slate Bandits & Meta-Experiments" ;;
      5|5.5) LABEL="sprint-5.5"; MS="Sprint 5.5: Advanced & Integration" ;;
      6|5.6) LABEL="sprint-5.6"; MS="Sprint 5.6: Metric Definition Layer" ;;
      I.0) LABEL="sprint-I.0"; MS="Sprint I.0: Scaffold + Foundation" ;;
      I.1) LABEL="sprint-I.1"; MS="Sprint I.1: Services + Wiring" ;;
      I.2) LABEL="sprint-I.2"; MS="Sprint I.2: Integration + Hardening" ;;
      I.3) LABEL="sprint-I.3"; MS="Sprint I.3: Multi-Cloud Foundation" ;;
      tc.0) LABEL="sprint-tc-0"; MS="TC.0: Foundations" ;;
      tc.1) LABEL="sprint-tc-1"; MS="TC.1: Statistical Goldens" ;;
      tc.2) LABEL="sprint-tc-2"; MS="TC.2: Service Binaries" ;;
      tc.3) LABEL="sprint-tc-3"; MS="TC.3: Contract Backfill" ;;
      tc.4) LABEL="sprint-tc-4"; MS="TC.4: UI E2E + Hygiene" ;;
      *) echo "Unknown sprint: {{sprint_num}}. Use 0-6, 5.0-5.6, I.0-I.3, or tc.0-tc.4."; exit 1 ;;
    esac
    echo "=== Launching workers for: $MS ==="
    # Use _ready to filter blocked or in-flight issues.
    ISSUES=$(just _ready "$LABEL")
    if [ -z "$ISSUES" ]; then
      # Distinguish "no work ready" (all blocked or in-flight) from "no Blocked-by
      # structure exists" (legacy sprint). If at least one labeled issue has a
      # "## Blocked by" section, _ready is authoritative — empty means "wait."
      # If NO labeled issue has the structure, fall back to legacy label/milestone.
      HAS_STRUCTURE=$(gh issue list --label "$LABEL" --state open --limit 50 --json body \
        --jq '[.[] | select(.body | contains("## Blocked by"))] | length' 2>/dev/null || echo "0")
      if [ "$HAS_STRUCTURE" = "0" ]; then
        ISSUES=$(gh issue list --label "$LABEL" --state open --json number,title --jq '.[] | @json' 2>/dev/null)
        if [ -z "$ISSUES" ]; then
          echo "  No issues found with label '$LABEL', trying milestone..."
          ISSUES=$(gh issue list --milestone "$MS" --state open --json number,title --jq '.[] | @json' 2>/dev/null)
        fi
      else
        echo "  ⚠ No ready issues for sprint {{sprint_num}}: every labeled issue is either blocked by an open dependency or has an in-flight PR. Wait for review/merge, then re-run."
        exit 0
      fi
    fi
    if [ -z "$ISSUES" ]; then
      echo "  ⚠ No issues found for sprint {{sprint_num}} via label or milestone. Nothing to launch."
      exit 0
    fi
    COUNT=0
    SKIPPED=0
    while IFS= read -r line; do
      num=$(echo "$line" | jq -r '.number')
      title=$(echo "$line" | jq -r '.title')
      echo "  → Issue #$num: $title"
      # Branch convention varies by sprint type; carried into the dispatch prompt.
      if [[ "$LABEL" == sprint-I.* ]]; then
        HINT="use infra-N/feat/description naming"
      elif [[ "$LABEL" == sprint-tc-* ]]; then
        HINT="use agent-N/test/tc-NNN-slug naming (see test-coverage-improvement-plan.md for the exact slug)"
      else
        HINT="use agent-N/feat/adr-XXX-description naming"
      fi
      rc=0
      ORCH_BRANCH_HINT="$HINT" bash scripts/orchestration/dispatch.sh "$num" multiclaude || rc=$?
      case "$rc" in
        0) COUNT=$((COUNT + 1)) ;;
        3) SKIPPED=$((SKIPPED + 1)); echo "    (already claimed — skipped)" ;;
        *) echo "    ✗ dispatch failed for #$num (rc=$rc)" ;;
      esac
    done <<< "$ISSUES"
    echo "✓ Workers launched for $MS ($COUNT dispatched, $SKIPPED already claimed)"
    echo "Monitor: just autonomous-status"

# --- Solo Mode ---

solo task_name="phase5-work":
    claude --worktree "{{task_name}}"

# --- Status & Diagnostics ---

phase5-status:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "=== Kaizen Phase 5 Status ==="
    echo ""
    echo "--- Gas Town ---"
    cd "$(eval echo {{kaizen_repo}})" && gt status 2>/dev/null || echo "  Not running"
    echo ""
    echo "--- Multiclaude ---"
    multiclaude status 2>/dev/null || echo "  Not running"
    echo ""
    echo "--- GitHub Issues ---"
    just sprint-status
    echo ""
    echo "--- Open PRs ---"
    gh pr list --limit 10 2>/dev/null || echo "  (gh not configured)"

# List Actions secrets on the current origin repo (names + last-updated only)
check-secrets:
    #!/usr/bin/env bash
    set -euo pipefail
    # Resolves the repo from the origin remote (via gh) rather than hardcoding it.
    # The GitHub API never returns secret VALUES — only names and last-updated
    # timestamps — so this is safe to run and share. Handy for confirming a
    # rotation took effect: check the "Updated" column (e.g. DOCKERHUB_TOKEN).
    # Requires admin/maintain access to the repo.
    if ! command -v gh >/dev/null 2>&1; then
      echo "✗ gh CLI not found — install from https://cli.github.com/" >&2
      exit 1
    fi
    REPO=$(gh repo view --json nameWithOwner -q .nameWithOwner 2>/dev/null) || {
      echo "✗ Could not resolve the origin repository. Are you in a gh-authenticated clone?" >&2
      exit 1
    }
    echo "=== Actions secrets on ${REPO} ==="
    echo "(values are write-only; only names + last-updated are shown)"
    echo ""
    gh secret list --repo "$REPO"

# --- PR Management ---

stop-all:
    #!/usr/bin/env bash
    cd "$(eval echo {{kaizen_repo}})" && gt down 2>/dev/null || true
    multiclaude worker kill --all 2>/dev/null || true
    multiclaude stop 2>/dev/null || true
    echo "✓ Everything stopped"