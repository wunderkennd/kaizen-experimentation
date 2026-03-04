# ADR-002: LMAX-Inspired Single-Threaded Policy Core for Bandit Service

## Status
Accepted

## Date
2026-03-03

## Context
The Bandit Policy Service (M4b) maintains mutable state: posterior parameters (Thompson Sampling), Cholesky decompositions (LinUCB), and neural network weights (Neural Contextual). This state is read on every SelectArm request (~10K rps) and mutated on every RewardEvent (~5K rps). Concurrent access to this state is the core design challenge.

Pattern observed in NautilusTrader: the kernel processes all messages on a single thread, with async I/O on separate threads feeding events via channels. This eliminates all synchronization primitives on the critical state.

## Decision
Adopt a three-thread architecture inspired by NautilusTrader's LMAX Disruptor pattern:

1. **Thread 1 (tokio async)**: gRPC server receives SelectArm requests. Sends `(context, oneshot_tx)` into a bounded `policy_channel`.
2. **Thread 2 (tokio async)**: Kafka consumer receives RewardEvents from `reward_events` topic. Sends events into a bounded `reward_channel`.
3. **Thread 3 (dedicated, single-threaded)**: Policy core event loop. Uses `select!` to receive from both channels. Performs all state mutations: posterior updates, Sherman-Morrison rank-1 updates, model weight changes, RocksDB snapshot writes. Returns arm selections via oneshot channels.

## Alternatives Considered
- **Actor model (Actix/Tokio actors)**: Provides message-passing concurrency but actors can still have complex lifecycle and supervision issues. The policy core is a single stateful entity, not a swarm of actors — actor model adds abstraction without benefit.
- **RwLock on shared state**: Read-heavy workload (10K reads vs 5K writes) suggests RwLock could work. However, LinUCB's Sherman-Morrison update modifies the Cholesky decomposition in-place — a write that takes microseconds but must be atomic with respect to reads. RwLock writer starvation under high read load is a real risk.
- **Lock-free data structures**: Crossbeam provides lock-free queues, but the policy state is not a simple counter — it's a matrix decomposition. Lock-free updates to a Cholesky factor are not practical.
- **Sharded policies (one per experiment)**: Eliminates cross-experiment contention but adds complexity for experiments with shared context features. Single-threaded core is simpler and sufficient at 15K combined rps.

## Consequences
- Zero mutex contention on policy state (verified via tokio-console in load tests).
- Backpressure is natural: full channels cause gRPC requests to queue at tokio layer, detectable by load balancer.
- Single-threaded core is the throughput bottleneck — if 15K rps is exceeded, must shard by experiment_id across multiple policy core instances.
- Channel depth metrics (policy_channel_depth, reward_channel_depth) are critical operational signals.
