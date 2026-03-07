#!/bin/bash
# init-test-db.sh — Mounted into postgres docker-entrypoint-initdb.d for CI.
# Handles FK ordering in 001_schema.sql via two-pass execution, then seeds
# reference data required by integration tests.
set -uo pipefail

echo "=== Applying schema migrations (two-pass for FK ordering) ==="
# Pass 1: suppress errors (FK-dependent tables fail, expected)
for f in /sql/migrations/*.sql; do
  psql -U "$POSTGRES_USER" -d "$POSTGRES_DB" -f "$f" 2>/dev/null || true
done
# Pass 2: show errors (everything should succeed now)
for f in /sql/migrations/*.sql; do
  psql -U "$POSTGRES_USER" -d "$POSTGRES_DB" -f "$f" || true
done

echo "=== Seeding reference data ==="
psql -U "$POSTGRES_USER" -d "$POSTGRES_DB" -f /sql/seed_dev.sql
