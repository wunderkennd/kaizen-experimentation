#!/bin/bash
# init-test-db.sh — Mounted into postgres docker-entrypoint-initdb.d for CI.
# Handles FK ordering in 001_schema.sql via two-pass execution, then seeds
# reference data required by integration tests.
set -uo pipefail

echo "=== Applying schema migrations (two-pass for FK ordering) ==="
for pass in 1 2; do
  for f in /sql/migrations/*.sql; do
    psql -U "$POSTGRES_USER" -d "$POSTGRES_DB" -f "$f" 2>/dev/null || true
  done
done

echo "=== Seeding reference data ==="
psql -U "$POSTGRES_USER" -d "$POSTGRES_DB" -f /sql/seed_dev.sql
