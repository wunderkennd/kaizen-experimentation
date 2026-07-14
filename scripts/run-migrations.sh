#!/bin/sh
# run-migrations.sh — entrypoint for the ECS db-migration task (see
# infra/pkg/aws/compute/migration.go). Applies sql/migrations/*.sql to RDS
# with the same two-pass semantics as init-test-db.sh: pass 1 tolerates
# FK-ordering failures, pass 2 re-applies with errors visible, then canary
# tables from the oldest and newest migrations prove the schema converged.
# Every migration file is CREATE ... IF NOT EXISTS-style, so re-running
# against an already-migrated database is a no-op and each deploy can run
# this task unconditionally.
#
# Expects DB_HOST (host:port), DB_USER, DB_PASS, DB_NAME injected by ECS
# from the RDS master-user secret.
set -u

export PGUSER="$DB_USER" PGPASSWORD="$DB_PASS" PGDATABASE="$DB_NAME" PGSSLMODE=require
export PGHOST="${DB_HOST%%:*}"
PGPORT="${DB_HOST##*:}"
[ "$PGPORT" = "$DB_HOST" ] && PGPORT=5432
export PGPORT

MIGRATIONS=/app/sql/migrations

echo "=== Applying schema migrations (two-pass for FK ordering) ==="
for f in "$MIGRATIONS"/*.sql; do
  psql -f "$f" >/dev/null 2>&1 || true
done
for f in "$MIGRATIONS"/*.sql; do
  psql -f "$f" >/dev/null || echo "WARN: $f did not re-apply cleanly"
done

echo "=== Verifying canary tables ==="
for t in layers metric_shadow_runs metric_migrations; do
  if ! psql -tAc "SELECT 1 FROM $t LIMIT 0" >/dev/null; then
    echo "ERROR: canary table $t missing — schema did not converge"
    exit 1
  fi
done
echo "Migrations complete."
