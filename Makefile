# ==============================================================================
# Experimentation Platform — Top-Level Makefile
# ==============================================================================
# Usage:
#   make setup       — Full local setup (infra + codegen + verify)
#   make test        — Run all tests (Rust + Go + TypeScript + hash parity)
#   make dev         — Start infra + seed data for local development
#   make clean       — Tear down all local infra and generated code
# ==============================================================================

.PHONY: setup dev clean test test-rust test-go test-ts test-hash \
        infra infra-down codegen codegen-go codegen-ts seed lint \
        docker-build bench monitoring monitoring-down help

SHELL := /bin/bash
.DEFAULT_GOAL := help

# ------------------------------------------------------------------------------
# Configuration
# ------------------------------------------------------------------------------
BUF       := buf
CARGO     := cargo
GO        := go
NPM       := npm
K6        := k6
DOCKER    := docker compose

PROTO_DIR    := proto
GEN_GO_DIR   := gen/go
GEN_TS_DIR   := gen/ts
SERVICES_DIR := services
CRATES_DIR   := crates
UI_DIR       := ui

# Load .env if present
-include .env
export

# ------------------------------------------------------------------------------
# Setup & Development
# ------------------------------------------------------------------------------

## setup: Full first-time setup — infra, codegen, install deps, verify
setup: infra codegen deps seed test
	@echo ""
	@echo "============================================"
	@echo "  Setup complete. All tests passing."
	@echo "============================================"

## dev: Start infra + seed data (skip tests for fast iteration)
dev: infra seed
	@echo ""
	@echo "  Infrastructure running. Seed data loaded."
	@echo "  Kafka UI:     http://localhost:8080"
	@echo "  Schema Reg:   http://localhost:8081"
	@echo "  PostgreSQL:   localhost:5432"
	@echo "  Redis:        localhost:6379"
	@echo ""

## clean: Tear down infra, remove generated code and build artifacts
clean: infra-down monitoring-down
	rm -rf $(GEN_GO_DIR) $(GEN_TS_DIR)
	rm -rf $(CRATES_DIR)/target
	rm -rf $(UI_DIR)/node_modules $(UI_DIR)/.next
	@echo "  Cleaned."

# ------------------------------------------------------------------------------
# Infrastructure
# ------------------------------------------------------------------------------

## infra: Start local infrastructure (Postgres, Kafka, Redis, Schema Registry)
infra:
	@echo "  Starting infrastructure..."
	$(DOCKER) up -d --wait
	@echo "  Infrastructure healthy."

## infra-down: Stop and remove infrastructure (preserves volumes)
infra-down:
	$(DOCKER) down

## infra-reset: Stop infrastructure and destroy volumes
infra-reset:
	$(DOCKER) down -v
	@echo "  Volumes destroyed. Run 'make infra seed' to re-initialize."

# ------------------------------------------------------------------------------
# Code Generation (Protobuf)
# ------------------------------------------------------------------------------

## codegen: Generate Go + TypeScript code from proto schemas
codegen: codegen-check codegen-go codegen-ts
	@echo "  Code generation complete."

codegen-check:
	@command -v $(BUF) >/dev/null 2>&1 || { echo "  Error: 'buf' not found. Install: https://buf.build/docs/installation"; exit 1; }

## codegen-go: Generate Go ConnectRPC stubs
codegen-go:
	@echo "  Generating Go stubs..."
	@mkdir -p $(GEN_GO_DIR)
	cd $(PROTO_DIR) && $(BUF) generate --template buf.gen.yaml --path experimentation

## codegen-ts: Generate TypeScript ConnectRPC stubs
codegen-ts:
	@echo "  Generating TypeScript stubs..."
	@mkdir -p $(GEN_TS_DIR)
	cd $(PROTO_DIR) && $(BUF) generate --template buf.gen.yaml --path experimentation

## lint-proto: Lint proto schemas and check for breaking changes
lint-proto:
	cd $(PROTO_DIR) && $(BUF) lint
	@echo "  Proto lint passed."

# ------------------------------------------------------------------------------
# Dependencies
# ------------------------------------------------------------------------------

## deps: Install all project dependencies
deps: deps-rust deps-go deps-ts

deps-rust:
	@echo "  Checking Rust toolchain..."
	@cd $(CRATES_DIR) && $(CARGO) check --workspace 2>/dev/null || $(CARGO) fetch --manifest-path $(CRATES_DIR)/Cargo.toml

deps-go:
	@echo "  Installing Go dependencies..."
	@cd $(SERVICES_DIR) && $(GO) mod download

deps-ts:
	@echo "  Installing TypeScript dependencies..."
	@cd $(UI_DIR) && $(NPM) ci --prefer-offline 2>/dev/null || $(NPM) install

# ------------------------------------------------------------------------------
# Testing
# ------------------------------------------------------------------------------

## test: Run all test suites
test: test-rust test-go test-ts test-hash
	@echo ""
	@echo "  All tests passed."

## test-rust: Run Rust workspace tests
test-rust:
	@echo "  Running Rust tests..."
	cd $(CRATES_DIR) && $(CARGO) test --workspace

## test-go: Run Go tests with race detection
test-go:
	@echo "  Running Go tests..."
	cd $(SERVICES_DIR) && $(GO) test -race -cover ./...

## test-ts: Run TypeScript tests
test-ts:
	@echo "  Running TypeScript tests..."
	cd $(UI_DIR) && $(NPM) test -- --passWithNoTests

## test-hash: Validate hash parity across implementations (10,000 vectors)
test-hash:
	@echo "  Validating hash parity..."
	python3 scripts/verify_hash_parity.py

## test-integration: Run integration tests against local infra
test-integration: infra
	@echo "  Running integration tests..."
	cd $(SERVICES_DIR) && $(GO) test -race -tags=integration ./...

# ------------------------------------------------------------------------------
# Linting
# ------------------------------------------------------------------------------

## lint: Run all linters
lint: lint-proto lint-rust lint-go lint-ts

lint-rust:
	cd $(CRATES_DIR) && $(CARGO) clippy --workspace --all-features -- -D warnings

lint-go:
	cd $(SERVICES_DIR) && $(GO) vet ./...

lint-ts:
	cd $(UI_DIR) && $(NPM) run lint

# ------------------------------------------------------------------------------
# Benchmarks
# ------------------------------------------------------------------------------

## bench: Run Rust benchmarks (hash + stats)
bench:
	@echo "  Running benchmarks..."
	cd $(CRATES_DIR) && $(CARGO) bench --workspace

# ------------------------------------------------------------------------------
# Seed Data
# ------------------------------------------------------------------------------

## seed: Load development seed data into local Postgres
seed:
	@echo "  Loading seed data..."
	@PGPASSWORD=$${POSTGRES_PASSWORD:-localdev} psql \
		-h $${POSTGRES_HOST:-localhost} \
		-p $${POSTGRES_PORT:-5432} \
		-U $${POSTGRES_USER:-experimentation} \
		-d $${POSTGRES_DB:-experimentation} \
		-f scripts/seed_dev.sql \
		--quiet
	@echo "  Seed data loaded."

# ------------------------------------------------------------------------------
# Docker Build
# ------------------------------------------------------------------------------

## docker-build: Build all service Docker images
docker-build:
	@echo "  Building Docker images..."
	$(DOCKER) -f docker-compose.yml build
	# Rust services
	@for svc in assignment analysis pipeline policy; do \
		echo "  Building experimentation-$$svc..."; \
		docker build -t experimentation-$$svc -f $(CRATES_DIR)/experimentation-$$svc/Dockerfile .; \
	done
	# Go services
	@for svc in management metrics flags orchestration; do \
		echo "  Building experimentation-$$svc..."; \
		docker build -t experimentation-$$svc -f $(SERVICES_DIR)/$$svc/Dockerfile .; \
	done
	# UI
	docker build -t experimentation-ui -f $(UI_DIR)/Dockerfile .
	@echo "  All images built."

# ------------------------------------------------------------------------------
# Monitoring Stack
# ------------------------------------------------------------------------------

## monitoring: Start Grafana + Prometheus + Jaeger alongside main infra
monitoring:
	@echo "  Starting monitoring stack..."
	$(DOCKER) -f docker-compose.yml -f docker-compose.monitoring.yml up -d --wait
	@echo "  Grafana:      http://localhost:3000  (admin/admin)"
	@echo "  Prometheus:   http://localhost:9090"
	@echo "  Jaeger:       http://localhost:16686"

## monitoring-down: Stop monitoring stack
monitoring-down:
	-$(DOCKER) -f docker-compose.yml -f docker-compose.monitoring.yml down 2>/dev/null

# ------------------------------------------------------------------------------
# Load Testing
# ------------------------------------------------------------------------------

## loadtest: Run k6 load test against local services
loadtest:
	@command -v $(K6) >/dev/null 2>&1 || { echo "  Error: 'k6' not found. Install: https://k6.io/docs/get-started/installation/"; exit 1; }
	$(K6) run scripts/loadtest.js

# ------------------------------------------------------------------------------
# Help
# ------------------------------------------------------------------------------

## help: Show this help message
help:
	@echo ""
	@echo "Experimentation Platform — Available Commands"
	@echo "=============================================="
	@grep -E '^## ' $(MAKEFILE_LIST) | sed 's/^## /  /' | column -t -s ':'
	@echo ""
