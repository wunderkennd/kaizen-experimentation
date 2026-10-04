# ADR-032: Unit-ID Pseudonymization at Ingest with Per-Unit Key Erasure

**Status**: Proposed
**Date**: 2026-10-04
**Deciders**: Agent-2 (M2 Ingest), Agent-5 (M5 Management), Agent-3 (M3 Metrics / Delta)
**Cluster**: — (cross-cutting privacy / data lifecycle)

---

## Context

The 2026-09-16 privacy review ([#825](https://github.com/wunderkennd/kaizen-experimentation/issues/825)) found that the platform cannot honour a GDPR/CCPA/LGPD erasure request:

- Raw `user_id` flows unhashed from SDKs → M2 ingest (presence check only, `crates/experimentation-ingest/src/validation.rs`) → Kafka (`metric_events` is *keyed* by `user_id`, `kafka/topic_configs.sh`) → the per-user Delta tables in `delta/delta_lake_tables.sql` (`exposures`, `metric_events`, `reward_events`, `qoe_events`, `metric_summaries`, `interleaving_scores`) → `user_trajectories` (`sql/migrations/010_user_trajectories.sql`) and M3-derived tables such as `mlrate_features`.
- The Delta `TBLPROPERTIES` retention settings are `delta.logRetentionDuration` / `delta.deletedFileRetentionDuration` — transaction-log and tombstone retention, not a data TTL. Nothing expires event rows.
- No Delete / Erase / Purge RPC exists in any proto.

PR [#834](https://github.com/wunderkennd/kaizen-experimentation/pull/834) fixed the integration guide (which falsely claimed an M5 deletion endpoint) and removed raw `user_id` from `HeartbeatSessionizer` tracing. The erasure path itself still needs a design. Two were on the table:

**Option A — M5 deletion RPC that cascades.** A `DeleteUser` RPC removes the user's rows everywhere. In practice:
- *Kafka*: every data topic uses `cleanup.policy=delete`. Records cannot be removed individually; the only lever is waiting out retention (90–180 days). Switching to compaction would break the event-stream semantics M3 and M4b rely on.
- *Delta*: tables are partitioned by date (and event type / experiment), not by user. Each erasure is a `DELETE WHERE user_id = ?` that rewrites data files in **every** partition the user ever touched, across at least eight tables, followed by `VACUUM`. At 100K events/s that is a full-table rewrite per request unless requests are batched, and batching widens the erasure SLA.
- *Postgres*: straightforward, but `user_trajectories` is the only Postgres table involved.
- Every new table that stores `user_id` has to join the cascade. A missed table is a silent compliance bug.

**Option B — pseudonymize at ingest, erase by destroying the key** (crypto-shredding). M2 replaces the raw identifier with a pseudonym derived from a per-unit secret before anything is published or stored. Erasure deletes that secret, which leaves every stored row permanently unlinkable to the person. Kafka, Delta and Postgres are never rewritten.

A codebase check found no consumer that needs the raw identifier after ingest:
- M1 hashes the raw unit ID for bucketing, in the SDK or at request time, and does not persist it.
- M3 joins `exposures` ↔ `metric_events` ↔ `metric_summaries` on `user_id` equality only. Any consistent opaque string works.
- M5's filter-SQL validator already rejects `user_id IN (SELECT …)` subqueries (`crates/experimentation-management/src/validators/filter_sql.rs`). Metric definitions therefore cannot join against external user tables.
- M4a reads `user_id` from Delta only as a grouping key (`crates/experimentation-analysis/src/delta_reader.rs`).

A plain keyed hash (one global HMAC "pepper") was also considered and rejected. It hides raw IDs from casual readers, but anyone holding the pepper can recompute the pseudonym. Erasing one person would still need row deletion (Option A).

---

## Decision

Adopt **Option B with a per-unit secret vault**, and add a **data-TTL retention job** as an independent backstop.

### 1. Pseudonym vault (owned by M2)

A small Postgres table in M2's schema:

```sql
CREATE TABLE unit_pseudonym_keys (
    lookup_hash  BYTEA PRIMARY KEY,   -- HMAC-SHA256(pepper, raw_unit_id); raw ID is never stored
    secret       BYTEA NOT NULL,      -- 32 random bytes, generated on first sight
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_seen_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
```

- `pepper` is a single service secret held in the secrets manager, never in the repo or the database.
- Get-or-create is `INSERT … ON CONFLICT (lookup_hash) DO NOTHING` followed by a `SELECT`. This is race-safe across M2 replicas.
- The vault sits beside an in-process cache in M2 (moka, keyed by `lookup_hash`). The cache TTL is short (default 10 min), which caps how long a just-erased secret can keep being used by a replica that missed the eviction message.

### 2. What M2 rewrites

Before dedup bookkeeping, sessionization and Kafka publish, M2 derives:

| Field | Replacement |
| --- | --- |
| `user_id` (all event types, incl. `QoEEvent`, heartbeats, interleaving) | `base64url(HMAC-SHA256(secret, "kaizen.unit.v1"))[..22]` |
| `device_id` | `base64url(HMAC-SHA256(secret, "kaizen.device.v1\|" + device_id))[..22]` |

- The Kafka key for `metric_events` becomes the pseudonym. Partition co-location for session assembly is unchanged.
- `session_id`, `event_id` and `content_id` are not personal identifiers and pass through unchanged. Free-form attributes are already forbidden by the integration guide (§2.6).
- The proto contract does not change. The field still carries "the unit ID"; only its value is opaque downstream of M2.

### 3. Erasure flow

1. An operator or integrator calls a new **M5 `ForgetUnit(unit_id)`** RPC. It is RBAC-gated (admin role) and writes an audit row holding `lookup_hash`, requester and timestamp. The raw ID is never logged.
2. M5 publishes `{lookup_hash}` to a new **`unit_forgotten`** Kafka topic (compacted, small volume).
3. M2 consumes `unit_forgotten`, deletes the vault row and evicts the cache entry.
4. Once the vault row is gone, existing rows in Kafka, Delta and Postgres can no longer be linked to the person. If the same person returns, they get a fresh secret and are treated as a new unit.

**Erasure SLA:** effective within cache TTL (≤10 min) of M2 consuming the message. Vault **backups** must be retained for no longer than the erasure SLA promised to integrators (proposed: 30 days). Otherwise a restore would resurrect shredded secrets.

### 4. Retention TTL (independent backstop)

Add a daily M3 job that drops expired partitions (`DELETE WHERE date_partition < cutoff`) and then runs `VACUUM`. Proposed defaults are below; every value is configurable per table.

| Table(s) | Default data TTL |
| --- | --- |
| `exposures`, `metric_events`, `qoe_events` | 90 days |
| `reward_events` | 180 days (bandit replay) |
| `metric_summaries`, `interleaving_scores`, `mlrate_features`, `user_trajectories` | 365 days |
| Aggregate-only tables (`daily_treatment_effects`, `content_consumption`, `experiment_level_metrics`) | no TTL (no unit IDs) |

Raw-event TTL limits recomputing metrics from raw events for experiments longer than 90 days. Per-user daily aggregates in `metric_summaries` (365 days) still cover them.

### 5. Data written before rollout

Rows written before pseudonymization ships hold raw IDs. Those rows age out under the retention job within one TTL window. If an erasure request arrives during that window, the operator also runs a targeted `DELETE WHERE user_id = '<raw>'` against the raw-ID partitions. This step is documented in a runbook and stops once the oldest raw-ID partition has expired.

---

## Consequences

### Benefits

1. Erasure is O(1): one row delete, with no Kafka or Delta rewrites. The cost does not grow with the number of tables or partitions.
2. New tables that store `user_id` are covered automatically, because they only ever see pseudonyms.
3. Raw identifiers no longer reach Kafka, Delta, notebooks, exports or logs. That shrinks the exposure surface even without erasure requests.
4. The retention job bounds storage growth and data lifetime independently of erasure.

### Trade-offs

1. **Hot-path dependency on the vault.** A cache miss costs one Postgres round-trip, and a unit seen for the first time also costs an insert. The vault must be highly available. If the vault is unreachable, M2 fails closed (rejects with `UNAVAILABLE`, and SDKs retry). It never passes raw IDs through.
2. **No lookup by raw ID downstream.** Debugging "what did user X see" requires computing the pseudonym through a privileged M2 admin path, which is out of scope here.
3. **Erased-then-returning users split into two units.** This is negligible for analysis, but SRM and unit counts can shift by the erasure volume.
4. **Vault size** is one row (~100 bytes) per unit ever seen. Rows whose `last_seen_at` is older than the longest data TTL can be pruned, because their data has already expired.
5. **Pepper rotation** requires an offline re-key of `lookup_hash`. Rotation should be rare and is runbook-driven.

---

## Implementation Details

### Proto Schema

```protobuf
// proto/experimentation/management/v1/management_service.proto
rpc ForgetUnit(ForgetUnitRequest) returns (ForgetUnitResponse);

message ForgetUnitRequest {
  string unit_id = 1;   // raw unit ID as the integrator knows it
}
message ForgetUnitResponse {
  google.protobuf.Timestamp accepted_at = 1;
}
```

`unit_forgotten` Kafka payload: `{"lookup_hash": "<base64>", "requested_at": "<RFC3339>"}`. Adding a new RPC and message is backward-compatible (`buf breaking` clean).

### Crate Layout / Public API

- `experimentation-ingest::pseudonym`: `Pseudonymizer` (vault client + cache) with `pseudonymize_unit(&str) -> Result<UnitPseudonym>` and `UnitPseudonym::device(&str) -> String`. The HMAC derivations are pure functions so they can be proptested.
- `experimentation-pipeline`: applies the `Pseudonymizer` in `process_event` / `process_batch_event` before publish, and runs the `unit_forgotten` consumer.
- `experimentation-management` (Rust M5) and `services/management` (Go M5, while it remains production): `ForgetUnit` handler + audit + publish.
- `services/metrics/internal/jobs/retention.go`: the TTL job.

### Integration

- **M2:** owns the vault, rewrites fields and consumes `unit_forgotten`.
- **M5:** owns `ForgetUnit`, RBAC and audit, and produces `unit_forgotten`.
- **M3:** owns the retention job. No join logic changes.
- **M1, M4a and M6:** no change.
- **M4b:** consumes pseudonymized `reward_events`. To verify during implementation: `SelectArm` carries the raw unit ID in-flight; confirm M4b does not persist it in RocksDB.

### Rollout

1. Ship the retention job (independent).
2. Ship the vault and `Pseudonymizer` behind `M2_PSEUDONYMIZE_UNITS=shadow`, which computes pseudonyms and records latency metrics but publishes raw values. Then switch to `enforce`.
3. Ship `ForgetUnit` + `unit_forgotten` + the M2 consumer, with an M5 → M2 contract test.
4. Update `docs/guides/integration/02-core-concepts.md` §2.6 and add the erasure runbook, then close #825.

---

## Validation

### Unit Tests / Proptest Invariants

- Determinism: the same secret and input always produce the same pseudonym.
- Separation: different secrets produce different pseudonyms for the same input, with overwhelming probability over 10K cases.
- Domain separation: unit and device derivations never collide for the same secret.
- No raw-ID leakage: published Kafka payloads and keys never contain the input `user_id` or `device_id` (property test over generated events).

### Contract / Integration Tests

- M5 → M2 `unit_forgotten` wire-format contract test (written by M2, the consumer).
- End-to-end: ingest events for unit U, `ForgetUnit(U)`, then ingest again. The new pseudonym differs from the old one, and the vault no longer holds `lookup_hash(U)`.
- Retention job: golden SQL for the generated `DELETE` / `VACUUM` statements per table.

### Accept / kill criteria

- **Accept:** in shadow mode, p99 ingest latency increases by less than 1 ms at the I.x load-test profile, with a vault cache hit rate of at least 99%.
- **Kill / revisit:** if the vault cannot meet the availability target alongside M2, fall back to Option A for erasure while keeping the global-pepper hash for exposure reduction.
