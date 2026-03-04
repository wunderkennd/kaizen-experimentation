# ADR-001: Language Selection — Rust for Hot Paths, Go for Orchestration, TypeScript for UI Only

## Status
Accepted

## Date
2026-03-03

## Context
The platform requires three categories of work: low-latency request handling and numerical computation (assignment, analysis, bandit policy, event ingestion), orchestration and CRUD (experiment management, metric job scheduling, feature flags), and browser-rendered visualization (dashboards, charts). We need to select languages that match each workload's characteristics.

## Decision
- **Rust** for M1 (Assignment), M2 (Event Ingestion), M4a (Statistical Analysis), M4b (Bandit Policy). These are hot-path services where p99 latency matters (assignment < 5ms, bandit arm selection < 15ms) and where numerical correctness is critical (statistical computation, hash determinism). Rust's zero-cost abstractions, absence of GC pauses, and strong type system prevent the categories of bugs most dangerous here: data races in bandit policy state, silent numerical overflow in bootstrap accumulators, and GC-induced latency spikes on assignment requests.
- **Go** for M3 (Metric Engine), M5 (Management), M7 (Feature Flags). These are orchestration services where development velocity and ecosystem maturity matter more than raw latency. Go's connect-go framework, excellent Kafka/Spark/PostgreSQL client libraries, and straightforward concurrency model suit CRUD APIs and job scheduling.
- **TypeScript** exclusively for M6 (Decision Support UI) — browser-rendered React dashboards. TypeScript never performs statistical computation, bandit policy evaluation, metric aggregation, or any backend processing.

## Alternatives Considered
- **All Go**: Simpler hiring, but Go's numerical computing ecosystem is immature. No equivalent to nalgebra, statrs, or tch-rs. GC pauses unacceptable for assignment service p99 targets.
- **All Rust**: Best performance, but Rust's compile times and learning curve slow down CRUD service development where the bottleneck is database I/O, not CPU. Go's ecosystem for web services (connect-go, sqlx, confluent-kafka-go) is more mature.
- **Python for analysis**: Common in data science, but single-threaded GIL limits throughput. Rust's proptest and fail-fast panics provide stronger correctness guarantees than Python's runtime type system.
- **Node.js for backend**: Would unify with TypeScript UI, but lacks the numerical libraries and performance characteristics needed for analysis and assignment services.

## Consequences
- Agents must be proficient in two languages minimum (Rust + Go or Go + TypeScript).
- Cross-language interop required for hash consistency (CGo bridge via experimentation-ffi crate).
- Two build systems (Cargo + Go modules) increase CI complexity.
- Proto schema serves as the language-neutral contract layer.
