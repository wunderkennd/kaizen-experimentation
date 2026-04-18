# ADR-003: RocksDB for Bandit Policy State (Crash-Only Design)

## Status
Accepted

## Date
2026-03-03

## Context
The Bandit Policy Service (M4b) needs durable state for crash recovery. The crash-only design principle (from NautilusTrader) requires that the recovery path and the startup path are identical: load last snapshot, replay events from Kafka. State must be persisted on every reward update as a side effect of normal operation — no separate "save on shutdown" path.

## Decision
Use embedded RocksDB for policy state snapshots. On every reward update, the single-threaded policy core writes a snapshot (serialized PolicySnapshot proto) to RocksDB keyed by `{experiment_id}:{timestamp}`. On startup/crash recovery, the core loads the latest snapshot per experiment and replays reward events from Kafka starting at the snapshot's `kafka_offset`.

## Alternatives Considered
- **Redis**: External dependency adds network latency on every write (~1ms vs ~10μs for RocksDB). Redis persistence (RDB/AOF) is asynchronous — could lose the last few seconds of state on crash, requiring more Kafka replay. Embedded RocksDB is synchronous and co-located.
- **PostgreSQL**: Too high latency for per-reward writes (~5ms). Acceptable for periodic archival snapshots (which we do separately for long-term audit) but not for the crash-only continuous-write pattern.
- **In-memory only (Kafka replay from beginning)**: No RocksDB dependency, but full replay of millions of rewards on every restart would take minutes, violating the <10 second crash recovery SLA.
- **Sled (Rust embedded KV)**: Promising but less battle-tested than RocksDB. RocksDB has proven production use in NautilusTrader and thousands of other systems.

## Consequences
- M4b has a disk dependency (RocksDB data directory). Kubernetes PersistentVolume required.
- Snapshot pruning needed: keep last N snapshots per experiment (default 10) to bound disk usage.
- PostgreSQL receives periodic archival snapshots (hourly) for long-term auditability, but is not on the critical recovery path.
