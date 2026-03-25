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
test: test-rust test-go test-ts test-hash
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