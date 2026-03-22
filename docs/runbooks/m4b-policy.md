# M4b Bandit Policy Service — Operational Runbook

## Service Overview

**Binary**: `experimentation-policy` (Rust)
**Port**: 50054 (gRPC)
**Profile**: Real-time — p99 < 15ms at 10K rps
**State**: Stateful — RocksDB for policy parameters, Kafka for reward ingestion
**Recovery**: Load RocksDB snapshot + replay Kafka from offset (SLA: < 10s)

### Architecture (LMAX pattern)

```
gRPC handlers (tokio async, multi-threaded)
        │
        ▼
    policy_tx (bounded mpsc, depth: 10K)
        │
        ▼
  ┌─────────────────────────────┐
  │  PolicyCore (single thread) │  ← All mutable state lives here
  │    ├── experiments: HashMap │
  │    ├── Thompson posteriors  │
  │    ├── LinUCB matrices      │
  │    └── RocksDB snapshots    │
  └─────────────────────────────┘
        ▲
        │
    reward_tx (bounded mpsc, depth: 50K)
        │
  Kafka consumer (reward_events topic)
```

**Key invariant**: No locks. No shared mutable state. Single-threaded core receives commands via channels, processes them sequentially, writes snapshots to RocksDB.

### RPCs

| RPC | Hot path | Purpose |
|-----|----------|---------|
| `SelectArm` | Yes (10K rps) | Select arm for a user (Thompson/LinUCB) |
| `CreateColdStartBandit` | No | Create new experiment for content |
| `ExportAffinityScores` | No | Export learned segment preferences |
| `GetPolicySnapshot` | No | Read current policy state |
| `RollbackPolicy` | No | Revert to previous snapshot |

### Dependencies

| Dependency | Required | Failure mode |
|------------|----------|--------------|
| RocksDB (local disk) | Yes | Cannot persist snapshots; crash = data loss |
| Kafka (`reward_events`) | Yes (for learning) | Policy stops updating; selections use stale parameters |

---

## Configuration

| Env var | Default | Description |
|---------|---------|-------------|
| `POLICY_GRPC_ADDR` | `[::1]:50054` | gRPC listen address |
| `POLICY_ROCKSDB_PATH` | — | RocksDB data directory |
| `POLICY_CHANNEL_DEPTH` | `10000` | SelectArm channel capacity |
| `REWARD_CHANNEL_DEPTH` | `50000` | Reward update channel capacity |
| `SNAPSHOT_INTERVAL` | `5` | Snapshot every N reward updates |
| `MAX_SNAPSHOTS_PER_EXPERIMENT` | `3` | RocksDB snapshot retention |
| `KAFKA_BROKERS` | `localhost:9092` | Kafka broker addresses |
| `KAFKA_GROUP_ID` | — | Consumer group ID |
| `KAFKA_REWARD_TOPIC` | `reward_events` | Kafka topic for rewards |
| `RUST_LOG` | `info` | Log level |

### Startup

```bash
POLICY_GRPC_ADDR="[::1]:50054" \
POLICY_ROCKSDB_PATH=/data/rocksdb/policy \
POLICY_CHANNEL_DEPTH=10000 \
REWARD_CHANNEL_DEPTH=50000 \
SNAPSHOT_INTERVAL=5 \
MAX_SNAPSHOTS_PER_EXPERIMENT=3 \
KAFKA_BROKERS=kafka:9092 \
KAFKA_GROUP_ID=policy-prod \
KAFKA_REWARD_TOPIC=reward_events \
./experimentation-policy
```

---

## Health Checks

### Quick probe

```bash
# Should return NotFound (service is healthy, experiment doesn't exist)
grpcurl -plaintext [::1]:50054 \
  experimentation.bandit.v1.BanditPolicyService/SelectArm \
  -d '{"experiment_id":"healthcheck","user_id":"probe"}'
```

### Prometheus metrics

```bash
curl -s http://localhost:50054/metrics | grep -E 'policy_(select_arm|channel|snapshot|errors)'
```

### Check policy state for an experiment

```bash
grpcurl -plaintext [::1]:50054 \
  experimentation.bandit.v1.BanditPolicyService/GetPolicySnapshot \
  -d '{"experiment_id":"exp-123"}'
```

---

## Alert Response Procedures

### PolicySelectArmLatencyHigh (CRITICAL)

**What**: SelectArm p99 exceeds the 15ms SLA.

**Why it matters**: M1 Assignment Service depends on SelectArm for real-time arm selection. High latency cascades to user-facing response times.

**Response**:
1. Check channel backpressure:
   ```bash
   curl -s http://localhost:50054/metrics | grep policy_channel_pending
   ```
2. If channel is >80% full → the core thread is the bottleneck:
   - Check for large LinUCB matrix inversions (high-dimensional contexts)
   - Check RocksDB write latency: `curl -s http://localhost:50054/metrics | grep snapshot_duration`
   - Reduce `SNAPSHOT_INTERVAL` (less frequent snapshots = less I/O)
3. If channel is low → network or gRPC layer issue:
   - Check TCP connection count: `ss -s | grep estab`
   - Check tokio runtime thread count
4. For sustained issues, use PGO-optimized binary: `just pgo-build-policy`

### PolicyChannelBackpressure (WARNING)

**What**: The policy channel (SelectArm requests) is >80% full.

**Why it matters**: When the channel fills, gRPC handlers block, causing timeout cascades.

**Response**:
1. Check if request rate has spiked: `curl -s http://localhost:50054/metrics | grep requests_total`
2. Check if the core thread is blocked on RocksDB I/O
3. Short-term: Increase `POLICY_CHANNEL_DEPTH` and restart
4. Long-term: Profile the core loop for slow operations

### PolicySnapshotFailed (CRITICAL)

**What**: RocksDB snapshot write failed.

**Why it matters**: Without snapshots, a crash means full Kafka replay from earliest offset, potentially violating the 10s recovery SLA.

**Response**:
1. Check disk space: `df -h $(dirname $POLICY_ROCKSDB_PATH)`
2. Check RocksDB health:
   ```bash
   ls -la $POLICY_ROCKSDB_PATH/
   # Should see SST files, MANIFEST, CURRENT
   ```
3. Common causes:
   - **Disk full**: RocksDB write amplification (~10x). Clean old snapshots or expand disk.
   - **Permissions**: Process lost write access to RocksDB directory.
   - **Corruption**: Delete RocksDB directory and restart (will replay from Kafka).

### PolicyUpdateStale (WARNING)

**What**: No reward updates in >10 minutes.

**Response**:
1. Check Kafka consumer status:
   ```bash
   kafka-consumer-groups.sh --bootstrap-server kafka:9092 \
     --group policy-prod --describe
   ```
2. Check if `reward_events` topic has new messages:
   ```bash
   kafka-console-consumer.sh --bootstrap-server kafka:9092 \
     --topic reward_events --max-messages 5 --from-latest
   ```
3. Common causes:
   - M2 pipeline is down (no events being produced)
   - Kafka broker is unreachable
   - Consumer group rebalancing (check logs for `Rebalance` messages)

### PolicyServiceDown (CRITICAL)

**What**: Service is unreachable.

**Response**:
1. Check process: `pgrep experimentation-policy`
2. Check logs: `journalctl -u experimentation-policy --since "5 min ago"`
3. Common crash causes:
   - `assert_finite!()` panic (NaN in bandit computation)
   - RocksDB corruption on startup
   - OOM (check `dmesg | grep -i oom`)
4. Restart: `systemctl restart experimentation-policy`
5. Verify recovery:
   ```bash
   grpcurl -plaintext [::1]:50054 \
     experimentation.bandit.v1.BanditPolicyService/SelectArm \
     -d '{"experiment_id":"healthcheck","user_id":"probe"}'
   ```

### PolicyErrorRate (WARNING)

**What**: >1% of requests are failing.

**Response**:
1. Check error breakdown in logs:
   ```bash
   journalctl -u experimentation-policy | grep "ERROR" | tail -50
   ```
2. Common error types:
   - `NotFound`: Experiment doesn't exist (expected for misconfigured clients)
   - `Unavailable`: Core thread shutting down (transient during restart)
   - `Internal`: Bug in policy computation (investigate and fix)

---

## Policy Rollback

If an experiment's policy has degraded (e.g., after a bad reward stream):

```bash
# 1. List available snapshots (check RocksDB keys)
grpcurl -plaintext [::1]:50054 \
  experimentation.bandit.v1.BanditPolicyService/GetPolicySnapshot \
  -d '{"experiment_id":"exp-123"}'

# 2. Rollback to a specific snapshot
grpcurl -plaintext [::1]:50054 \
  experimentation.bandit.v1.BanditPolicyService/RollbackPolicy \
  -d '{"experiment_id":"exp-123","target_snapshot_epoch_ms":1710000000000}'
```

---

## Crash Recovery

The service follows crash-only design. Recovery on startup:

1. Open RocksDB at `POLICY_ROCKSDB_PATH`
2. Scan for latest snapshot per experiment
3. Deserialize policy parameters from each snapshot
4. Register experiments with restored state
5. Start Kafka consumer from committed offset
6. Resume processing

**Expected recovery time**: < 10 seconds (per SLA)

### Verifying recovery after crash

```bash
# 1. Check recovery time in logs
journalctl -u experimentation-policy --since "5 min ago" | grep -i "recover\|restore\|loaded"

# 2. Verify a known experiment returns arms
grpcurl -plaintext [::1]:50054 \
  experimentation.bandit.v1.BanditPolicyService/SelectArm \
  -d '{"experiment_id":"<known-experiment>","user_id":"recovery-probe"}'

# 3. Run chaos test to validate
just chaos-policy
```

### Full recovery from scratch (RocksDB lost)

If RocksDB is corrupted or deleted:
1. Delete the directory: `rm -rf $POLICY_ROCKSDB_PATH`
2. Restart the service
3. It will replay all rewards from Kafka (may take minutes for large histories)
4. Monitor recovery via `curl -s http://localhost:50054/metrics | grep policy_last_update_timestamp`

---

## Load Testing

```bash
# Standard SLA validation: p99 < 15ms at 10K rps
just loadtest-policy

# Custom parameters
TARGET_RPS=5000 DURATION=30 bash scripts/loadtest_policy.sh
```

---

## Performance Tuning

### Benchmarks

```bash
just bench-crate experimentation-bandit
```

Key benchmarks and typical ranges (release mode, Apple M-series):
- `thompson_select_arm_10`: ~500ns-2us
- `thompson_update_reward`: ~50-100ns
- `linucb_select_arm_10_d8`: ~5-15us
- `linucb_update_d8`: ~2-5us (Sherman-Morrison rank-1 update)

### RocksDB tuning

For policy snapshots (small, frequent writes):
- Reduce `max_write_buffer_number` (default 2 is fine)
- Reduce `target_file_size_base` (16MB → 4MB for snapshot workload)
- Write amplification is ~10x by default; monitor disk throughput

### Channel depth tuning

- `POLICY_CHANNEL_DEPTH=10000`: Start here. If backpressure alerts fire at peak load, increase.
- `REWARD_CHANNEL_DEPTH=50000`: Kafka rewards are bursty. Size for 10s of peak reward rate.
- Too large = memory waste + delayed backpressure signal. Too small = gRPC timeouts.

### PGO build

```bash
just pgo-build-policy  # 3-phase: instrument → profile → optimize
```
