# ============================================================================
# Experimentation Platform: Kafka Topic Configuration
# Applied via kafka-topics.sh or Terraform/Pulumi IaC
# ============================================================================

# --- exposures ---
# Source: M2 Event Ingestion (Rust)
# Consumers: M3 Metric Engine (Spark), M4a Analysis (SRM check)
# Key: experiment_id (colocates all exposures for one experiment on same partition)
# Volume estimate: ~50K events/sec peak (1M DAU, 50 experiments, 1 exposure/user/experiment/day)
kafka-topics.sh --create --topic exposures \
  --partitions 64 \
  --replication-factor 3 \
  --config retention.ms=7776000000 \       # 90 days
  --config cleanup.policy=delete \
  --config segment.bytes=1073741824 \      # 1 GB segments
  --config max.message.bytes=1048576 \     # 1 MB max message
  --config min.insync.replicas=2

# --- metric_events ---
# Source: M2 Event Ingestion (Rust)
# Consumers: M3 Metric Engine (Spark)
# Key: user_id (ensures all events for one user on same partition for session assembly)
# Volume estimate: ~100K events/sec peak (highest volume topic)
kafka-topics.sh --create --topic metric_events \
  --partitions 128 \
  --replication-factor 3 \
  --config retention.ms=7776000000 \       # 90 days
  --config cleanup.policy=delete \
  --config segment.bytes=1073741824 \
  --config max.message.bytes=1048576 \
  --config min.insync.replicas=2

# --- reward_events ---
# Source: M2 Event Ingestion (Rust)
# Consumers: M4b Bandit Policy Service (Rust, real-time), M3 Metric Engine (Spark, batch)
# Key: experiment_id (colocates rewards for one experiment for policy updates)
# Volume estimate: ~5K events/sec (only bandit experiments generate rewards)
# CRITICAL: M4b consumes this in real-time for policy updates.
# Consumer group: bandit-policy-service (committed offsets used for crash recovery replay)
kafka-topics.sh --create --topic reward_events \
  --partitions 32 \
  --replication-factor 3 \
  --config retention.ms=15552000000 \      # 180 days (longer for bandit replay)
  --config cleanup.policy=delete \
  --config segment.bytes=536870912 \       # 512 MB segments
  --config max.message.bytes=1048576 \
  --config min.insync.replicas=2

# --- qoe_events ---
# Source: M2 Event Ingestion (Rust)
# Consumers: M3 Metric Engine (Spark)
# Key: session_id (colocates all QoE for one playback session)
# Volume estimate: ~20K events/sec (one per playback session completion)
kafka-topics.sh --create --topic qoe_events \
  --partitions 64 \
  --replication-factor 3 \
  --config retention.ms=7776000000 \       # 90 days
  --config cleanup.policy=delete \
  --config segment.bytes=1073741824 \
  --config max.message.bytes=1048576 \
  --config min.insync.replicas=2

# --- guardrail_alerts ---
# Source: M3 Metric Engine (Go, when hourly guardrail check detects breach)
# Consumers: M5 Management Service (Go, triggers auto-pause)
# Key: experiment_id
# Volume estimate: ~10 events/hour (rare; only on breach)
# Low-latency: M5 must consume within 60 seconds for auto-pause SLA.
kafka-topics.sh --create --topic guardrail_alerts \
  --partitions 8 \
  --replication-factor 3 \
  --config retention.ms=2592000000 \       # 30 days
  --config cleanup.policy=delete \
  --config segment.bytes=268435456 \       # 256 MB segments
  --config max.message.bytes=1048576 \
  --config min.insync.replicas=2

# --- surrogate_recalibration_requests ---
# Source: M5 Management Service (Go, on TriggerSurrogateRecalibration RPC)
# Consumers: M3 Metric Engine (Go, triggers surrogate model recalibration)
# Key: model_id (colocates all requests for one model on same partition)
# Volume estimate: ~1 event/day (rare; manual or scheduled trigger)
kafka-topics.sh --create --topic surrogate_recalibration_requests \
  --partitions 4 \
  --replication-factor 3 \
  --config retention.ms=2592000000 \       # 30 days
  --config cleanup.policy=delete \
  --config segment.bytes=268435456 \       # 256 MB segments
  --config max.message.bytes=1048576 \     # 1 MB max message
  --config min.insync.replicas=2

# ============================================================================
# Consumer Groups
# ============================================================================
# metric-engine-spark       : M3 reads exposures, metric_events, qoe_events (batch)
# bandit-policy-service     : M4b reads reward_events (real-time, committed offsets for crash recovery)
# management-guardrail      : M5 reads guardrail_alerts (real-time, auto-pause trigger)
# metric-engine-surrogate   : M3 reads surrogate_recalibration_requests (on-demand recalibration)
# delta-lake-sink           : Kafka Connect writes all topics to Delta Lake

# ============================================================================
# Schema Registry
# ============================================================================
# All topics use Protobuf serialization (Confluent Schema Registry or Buf).
# Schema compatibility mode: BACKWARD (allows adding optional fields).
# Each topic is registered with its corresponding .proto message type:
#   exposures         -> experimentation.common.v1.ExposureEvent
#   metric_events     -> experimentation.common.v1.MetricEvent
#   reward_events     -> experimentation.common.v1.RewardEvent
#   qoe_events        -> experimentation.common.v1.QoEEvent
#   guardrail_alerts  -> experimentation.common.v1.GuardrailAlert
#   surrogate_recalibration_requests -> JSON (surrogate.RecalibrationRequest)
