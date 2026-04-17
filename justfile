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

# Full first-time setup — infra, codegen, install deps, seed data, verify
setup: infra codegen deps seed test
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

# Run Rust flags service tests (ADR-024: Go M7 deleted, Rust crate is the implementation)
test-flags:
    @echo "  Running Rust flags service tests..."
    {{ cargo }} test -p experimentation-flags

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
    @docker exec -it $$({{ docker }} ps -q --filter name=kafka 2>/dev/null | head -1) \
        kafka-topics --bootstrap-server localhost:29092 --list 2>/dev/null \
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
    jules remote new --repo your-org/kaizen \
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

# Launch a Multiclaude worker from a GitHub Issue number
work-on issue:
    #!/usr/bin/env bash
    set -euo pipefail
    TASK=$(gh issue view {{issue}} --json title,body -q '"\(.title)\n\n\(.body)"')
    echo "=== Launching worker for Issue #{{issue}} ==="
    echo "$TASK" | head -3
    echo "..."
    multiclaude worker create "$TASK. Branch: use the agent-N/feat/adr-XXX naming convention. PR must include 'Closes #{{issue}}'."

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
      I.0) LABEL="sprint-I.0"; MS="Sprint I.0: Scaffold + Foundation" ;;
      I.1) LABEL="sprint-I.1"; MS="Sprint I.1: Services + Wiring" ;;
      I.2) LABEL="sprint-I.2"; MS="Sprint I.2: Integration + Hardening" ;;
      *) echo "Unknown sprint: {{sprint_num}}. Use 0-5, 5.0-5.5, or I.0-I.2."; exit 1 ;;
    esac
    echo "=== Launching workers for: $MS ==="
    # Query by label first (always present), fall back to milestone
    # Use jq to produce one JSON object per line (handles multi-line bodies safely)
    ISSUES=$(gh issue list --label "$LABEL" --state open --json number,title --jq '.[] | @json' 2>/dev/null)
    if [ -z "$ISSUES" ]; then
      echo "  No issues found with label '$LABEL', trying milestone..."
      ISSUES=$(gh issue list --milestone "$MS" --state open --json number,title --jq '.[] | @json' 2>/dev/null)
    fi
    if [ -z "$ISSUES" ]; then
      echo "  ⚠ No open issues found for sprint {{sprint_num}}. Nothing to launch."
      exit 0
    fi
    COUNT=0
    echo "$ISSUES" | while IFS= read -r line; do
      num=$(echo "$line" | jq -r '.number')
      title=$(echo "$line" | jq -r '.title')
      # Fetch full issue body separately to avoid newline parsing issues
      body=$(gh issue view "$num" --json body -q '.body' 2>/dev/null | head -50)
      echo "  → Issue #$num: $title"
      # Use infra-N branch naming for IaC sprints, agent-N for Phase 5
      if [[ "$LABEL" == sprint-I.* ]]; then
        BRANCH_HINT="Branch: use infra-N/feat/description naming."
      else
        BRANCH_HINT="Branch: use agent-N/feat/adr-XXX naming."
      fi
      multiclaude worker create "$title. $body. $BRANCH_HINT PR must include 'Closes #$num'."
      COUNT=$((COUNT + 1))
    done
    echo "✓ Workers launched for $MS ($COUNT issues)"
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

# --- PR Management ---

stop-all:
    #!/usr/bin/env bash
    cd "$(eval echo {{kaizen_repo}})" && gt down 2>/dev/null || true
    multiclaude worker kill --all 2>/dev/null || true
    multiclaude stop 2>/dev/null || true
    echo "✓ Everything stopped"