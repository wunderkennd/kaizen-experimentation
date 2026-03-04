


EXPERIMENTATION PLATFORM
System Design & Agent Implementation Plan
SVOD-Native Architecture for Streaming Platforms
ConnectRPC · Go + Rust · Contextual Bandits · Interleaving · Open-Source Patterns
Version 5.1  —  March 2026
CONFIDENTIAL


# Table of Contents

1. Executive Summary
1.1 Design Principles
1.2 Open-Source Lineage
1.3 SVOD-Specific Capabilities
1.4 Language & Module Strategy
2. Open-Source Architectural Patterns
2.1 Cargo Workspace with Crate Layering
2.2 Crash-Only Design
2.3 LMAX-Inspired Single-Threaded Policy Core
2.4 Fail-Fast Data Integrity
2.5 Component State Machine
2.6 SDK Provider Abstraction
2.7 SQL Transparency & Notebook Export
2.8 Group Sequential Tests
2.9 Automated Bucket Reuse
2.10 Guardrails Default to Auto-Pause
3. Proto Schema Architecture
3.1 Repository Structure
3.2 Experiment State Machine
3.3 SVOD-Specific Protos
3.4 Sequential Testing Config
3.5 Guardrail Action Config
4. Module 1: Assignment Service (Rust)
5. Module 2: Event Pipeline (Rust + Go)
6. Module 3: Metric Computation Engine (Go)
7. Module 4a: Statistical Analysis Engine (Rust)
8. Module 4b: Bandit Policy Service (Rust)
9. Module 5: Experiment Management Service (Go)
10. Module 6: Decision Support UI (TypeScript, UI Only)
11. Module 7: Feature Flag Service (Go)
12. Cross-Cutting Concerns
13. Implementation Roadmap
13.1 Phase 0: Schema & Toolchain (Week 1)
13.2 Phase 1: Foundation (Weeks 2–7)
13.3 Phase 2: Analysis & UI (Weeks 6–11)
13.4 Phase 3: SVOD-Native + Bandits (Weeks 10–17)
13.5 Phase 4: Advanced & Polish (Weeks 16–22)
14. Agent Coordination Protocol
15. Appendix

Figures
Figure 1: System Architecture Overview (Section 1.4)
Figure 2: Cargo Workspace Crate Dependency Graph (Section 2.1)
Figure 3: LMAX-Inspired Bandit Policy Threading Model (Section 2.3)
Figure 4: Experiment Lifecycle State Machine (Section 2.5)
Figure 5: SDK Provider Fallback Chain (Section 2.6)
Figure 6: End-to-End Data Flow Pipeline (Section 5)


# 1. Executive Summary

This document provides a comprehensive system design for an in-house experimentation platform purpose-built for SVOD streaming platforms. Unlike general-purpose experimentation tools, this platform treats streaming-specific concerns as first-class architectural primitives: interleaving experiments, surrogate metrics for churn prediction, novelty effect detection, content catalog interference, subscriber lifecycle segmentation, session-level randomization, playback quality experimentation, and content cold-start bandits.
Version 5 incorporates architectural lessons from production open-source systems — most notably NautilusTrader (Rust/Python high-performance trading platform), GrowthBook (warehouse-native experimentation), and Spotify Confidence (statistical analysis at scale) — that directly inform our Rust crate structure, crash recovery design, threading model, SDK abstraction, and statistical validation strategy.


## 1.1 Design Principles

SVOD-Native: Streaming-specific experiment types (interleaving, session-level, playback QoE), metric taxonomies, and analysis methods are built in, not bolted on.
Adaptive: Contextual bandits and content cold-start bandits enable real-time optimization during experiments.
Predictive: Surrogate metric framework projects long-horizon outcomes (churn) from short-term signals.
Interference-Aware: Content catalog spillover detection and content holdout designs account for finite-catalog SVOD dynamics.
Crash-Only (from NautilusTrader): Stateless services share startup and recovery code paths. No separate graceful-shutdown logic that goes untested.
Fail-Fast (from NautilusTrader): Invalid data (NaN, overflow, negative durations) triggers immediate failure rather than silent propagation to treatment effect estimates.
Schema-First: All interfaces defined in Protobuf with buf toolchain enforcement.
Language-Appropriate: Rust for hot paths (assignment, ingestion, statistical computation, bandit policy). Go for orchestration (management, metric job scheduling, feature flags). TypeScript exclusively for browser-rendered UI and client-side SDK provider wrappers — TypeScript never performs statistical computation, bandit policy evaluation, or metric aggregation.
Crate-Layered (from NautilusTrader): Rust services share a Cargo workspace with focused crates, feature-flagged bindings, and explicit dependency boundaries.
SDK-Abstracted (from GrowthBook): Client SDKs implement a provider interface with local, remote, and mock backends for testability and resilience.
Guardrails-Default-Safe (from Spotify): Guardrail breaches auto-pause experiments by default; explicit override required to continue.


## 1.2 Open-Source Lineage


| Pattern | Source Project | Applied To |
| --- | --- | --- |
| Cargo workspace with crate layering & feature flags | NautilusTrader | All Rust services: shared core/hash/stats/bandit/ingest crates |
| Crash-only design with externalized state | NautilusTrader | Assignment Service, Event Ingestion (stateless crash recovery) |
| LMAX-inspired single-threaded event loop | NautilusTrader | Bandit Policy Service policy core (channel-fed, lock-free) |
| Fail-fast data integrity with property-based tests | NautilusTrader | Statistical Analysis Engine (proptest invariants) |
| Component state machine with transitional states | NautilusTrader | Experiment lifecycle (STARTING, CONCLUDING transitions) |
| SDK provider abstraction with fallback chain | GrowthBook | Client SDKs (Remote > Local cache > Static defaults) |
| SQL transparency and notebook export | GrowthBook | Metric Computation Engine (View SQL, Export to Notebook) |
| Group sequential tests alongside mSPRT | Spotify Confidence | Statistical Analysis Engine (GST for fixed-schedule reviews) |
| Automated bucket reuse on experiment conclusion | Spotify Confidence | Assignment Service layer allocation recycling |
| Guardrails default to auto-pause | Spotify Confidence | Experiment Management Service guardrail policy |


## 1.3 SVOD-Specific Capabilities


| Capability | Problem Solved | Module(s) |
| --- | --- | --- |
| Interleaving Experiments | A/B tests need millions of users; interleaving is 10-100x more sensitive | M1, M3, M4a, M5, M6 |
| Surrogate Metrics | Churn takes 30-90 days; surrogates predict impact in days | M3, M4a, M5, M6 |
| Novelty Effect Detection | Rec changes show transient spikes; prevents shipping based on fading lift | M4a, M6 |
| Content Interference | Finite catalog creates spillover between treatment/control | M3, M4a, M6 |
| Lifecycle Segmentation | Trial/new/established/at-risk subscribers respond differently | M1, M3, M4a, M5, M6 |
| Session-Level Experiments | Some questions are about sessions; requires clustered analysis | M1, M3, M4a, M5 |
| Playback QoE Experiments | ABR/CDN/encoding have distinct metrics (rebuffer, TTFF, bitrate) | M2, M3, M4a, M6 |
| Content Cold-Start Bandit | New content has no data; bandit learns optimal user targeting | M4b, M3, M6 |
| Cumulative Holdout Groups | Measures total algorithmic lift over time | M1, M4a, M5, M6 |


## 1.4 Language & Module Strategy


| Module | Language | Agent | Responsibility |
| --- | --- | --- | --- |
| M1: Assignment | Rust | Agent-1 | Variant allocation, interleaving list construction, bandit arm delegation |
| M2: Event Pipeline | Rust+Go | Agent-2 | Rust: event validation/dedup/Kafka publish. Go: job orchestration/alerting |
| M3: Metric Computation | Go | Agent-3 | Spark SQL orchestration: per-user metrics, interleaving scores, surrogates, QoE |
| M4a: Statistical Analysis | Rust | Agent-4 | All statistical computation: frequentist, mSPRT, GST, novelty, interference |
| M4b: Bandit Policy | Rust | Agent-4 | All bandit computation: Thompson, LinUCB, cold-start. LMAX single-thread core |
| M5: Experiment Mgmt | Go | Agent-5 | CRUD, lifecycle state machine, auto-pause guardrails, bucket reuse mgmt |
| M6: Decision Support UI | TypeScript (UI only) | Agent-6 | Frontend dashboards and visualization only. No backend computation. |
| M7: Feature Flags | Go | Agent-7 | Progressive delivery, flag evaluation, promotion to all experiment types |


Figure 1: System Architecture Overview
See accompanying file: system_architecture.mermaid
Module topology showing all seven modules, their implementation languages (Rust brown, Go blue, TypeScript green), inter-service communication paths (gRPC, Kafka), and infrastructure dependencies (Delta Lake, PostgreSQL, Redis, RocksDB). Arrows indicate data flow direction; dashed arrows indicate fallback paths.


# 2. Open-Source Architectural Patterns

This section documents specific architectural patterns drawn from production open-source systems and how they are applied throughout the platform. These are not abstract influences — they are concrete structural decisions with implementation-level specificity.


## 2.1 Cargo Workspace with Crate Layering (from NautilusTrader)

NautilusTrader organizes its Rust codebase into focused crates with explicit dependency boundaries: Foundation (core, model, common), Engines (data, execution), Infrastructure (serialization, network, persistence), Runtime (live, backtest), and Bindings (pyo3). Feature flags gate optional functionality at compile time. We adopt this pattern directly.

crates/
experimentation-core/      # Timestamps, config types, error types, tracing setup
experimentation-hash/      # MurmurHash3, bucketing logic
features: [wasm, uniffi, ffi, python]
experimentation-proto/     # tonic-build generated code from .proto files
experimentation-stats/     # Bootstrap, CUPED, mSPRT, GST, novelty detection
features: [simd, python]
experimentation-bandit/    # Thompson Sampling, LinUCB, Neural, cold-start
features: [gpu]  # tch-rs for neural bandits
experimentation-interleaving/  # Team Draft, Optimized Interleaving algorithms
experimentation-ingest/    # Event validation, dedup, Kafka publishing
experimentation-ffi/       # CGo bindings via cbindgen (for Go hash interop)
experimentation-assignment/ # Assignment service binary (depends on hash, proto, interleaving, bandit)
experimentation-analysis/  # Analysis service binary (depends on stats, proto)
experimentation-pipeline/  # Ingestion service binary (depends on ingest, proto)
experimentation-policy/    # Bandit policy service binary (depends on bandit, proto)

Feature flags control cross-language binding generation. Building with --features wasm compiles experimentation-hash to WebAssembly via wasm-bindgen for the browser SDK. Building with --features ffi generates C headers via cbindgen for the Go CGo bridge. Building with --features uniffi generates Swift and Kotlin bindings for mobile SDKs. The same Rust source produces all targets — no separate codebases.


| Crate Category | Crates | Purpose |
| --- | --- | --- |
| Foundation | core, hash, proto | Primitives, hashing, generated types |
| Algorithms | stats, bandit, interleaving | Statistical methods, policy algorithms, list construction |
| Infrastructure | ingest, ffi | Event processing, cross-language bindings |
| Services | assignment, analysis, pipeline, policy | Service binaries (thin wrappers over algorithm crates) |


Service binaries are thin orchestration shells. They handle gRPC/Connect serving, config loading, and observability, but delegate all domain logic to the algorithm crates. This means the core statistical and bandit algorithms can be unit-tested in isolation without spinning up a gRPC server — and can be reused in offline analysis tools, notebooks, or future services.

Figure 2: Cargo Workspace Crate Dependency Graph
See accompanying file: crate_graph.mermaid
Dependency flow from Foundation layer (core, hash, proto) through Algorithm layer (stats, bandit, interleaving) to Service Binaries (assignment, analysis, pipeline, policy). Arrows point from dependency to dependent. Service binaries are thin shells; all domain logic lives in the algorithm crates.


## 2.2 Crash-Only Design (from NautilusTrader)

NautilusTrader treats crash recovery as the primary recovery path: startup and crash recovery share the same code, critical state is externalized, and the system is designed for fast restart with idempotent operations. We apply this principle differentially based on statefulness:


| Service | State Model | Crash Recovery Strategy |
| --- | --- | --- |
| Assignment Service | Stateless (config from Management Service) | Restart, re-fetch config snapshot. Identical assignments immediately (deterministic hashing). |
| Event Ingestion | Stateless (Kafka producer) | Restart, reconnect to Kafka. At-least-once delivery; downstream dedup handles retries. |
| Statistical Analysis | Stateless (reads Delta Lake, writes results to Postgres) | Restart, re-run analysis job. Idempotent: same input produces same output. |
| Bandit Policy Service | Stateful (policy parameters in RocksDB) | Snapshot to RocksDB on every policy update. Recovery: load last snapshot, replay reward events from Kafka offset. No separate graceful-shutdown persistence path. |


The key insight: for the three stateless services, there is literally no warm-up state. A freshly-started process produces identical results to one that has been running for weeks. For the Bandit Policy Service, the crash-only principle means we do not have a separate 'save state on shutdown' path — state is persisted continuously to RocksDB as a side effect of normal operation. The recovery path (load snapshot + replay Kafka) is the same code as initial startup.


## 2.3 LMAX-Inspired Single-Threaded Policy Core (from NautilusTrader)

NautilusTrader processes all kernel messages on a single thread, inspired by the LMAX Disruptor pattern. Background I/O uses separate async runtimes that feed events into the core via channels. We apply this to the Bandit Policy Service, which has the most complex concurrency requirements:

// Bandit Policy Service threading model
//
// Thread 1 (tokio): gRPC server receives SelectArm requests
//   -> sends (context, oneshot_tx) into policy_channel
//
// Thread 2 (tokio): Kafka consumer receives RewardEvents
//   -> sends RewardEvent into reward_channel
//
// Thread 3 (dedicated): Policy Core event loop (SINGLE THREADED)
//   loop {
//     select! {
//       req = policy_channel.recv() => {
//         let arm = policy.select_arm(req.context);
//         req.response_tx.send(arm);
//       }
//       reward = reward_channel.recv() => {
//         policy.update(reward);  // Sherman-Morrison, posterior update
//         policy.snapshot_to_rocksdb();  // crash-only: persist on every update
//       }
//     }
//   }

This eliminates all locks and mutexes on the policy state. The Cholesky decomposition, posterior parameters, and model weights are owned by a single thread. Tokio threads handle I/O (gRPC serving, Kafka consumption) and communicate via bounded channels. Backpressure is natural: if the policy core cannot keep up, the channels fill and gRPC requests queue at the tokio layer, which the load balancer can detect.

Figure 3: LMAX-Inspired Bandit Policy Threading Model
See accompanying file: lmax_threading.mermaid
Three-thread architecture: Thread 1 (tokio) handles gRPC SelectArm requests, Thread 2 (tokio) consumes Kafka reward events, Thread 3 (dedicated) runs the single-threaded policy core event loop. Bounded channels connect I/O threads to the core. All state mutations (Thompson Sampling posterior, LinUCB Sherman-Morrison, RocksDB snapshots) occur on the dedicated thread with zero mutex contention.


## 2.4 Fail-Fast Data Integrity (from NautilusTrader)

NautilusTrader panics immediately on arithmetic overflow, NaN deserialization, and type conversion failures, reasoning that corrupt data cascading through a trading system is worse than downtime. We adopt the same philosophy for the Statistical Analysis Engine:

NaN or Infinity in any metric value: panic. A NaN in a CUPED covariate matrix would silently produce meaningless treatment effect estimates.
Negative watch-time durations: panic during event validation. Negative durations propagating to mean calculations would bias results.
Arithmetic overflow in bootstrap accumulators: checked_add with panic on overflow. Silent wraparound would produce nonsensical confidence intervals.
Division by zero in metric ratios: explicit check and Result::Err, not silent NaN propagation.
These panics are caught by the service's crash-only design: the process restarts, the corrupt event is identified in logs, and the analysis re-runs on clean data.

Property-based testing via proptest validates these invariants in CI:
// Example proptest invariant for CUPED
#[test]
fn cuped_never_increases_variance() {
proptest!(|(n in 1000..100000u64, rho in 0.1..0.99f64)| {
let (raw_var, cuped_var) = simulate_cuped(n, rho);
prop_assert!(cuped_var <= raw_var,
"CUPED increased variance: raw={}, cuped={}, rho={}",
raw_var, cuped_var, rho);
});
}


## 2.5 Component State Machine (from NautilusTrader)

NautilusTrader defines both stable states and transitional states for all components, preventing race conditions during state changes. We extend the experiment lifecycle with two transitional states:


| State | Type | Description |
| --- | --- | --- |
| DRAFT | Stable | Experiment configured but not yet validated or started. |
| STARTING | Transitional (NEW) | Validating config, warming up bandit policy, confirming metric availability, checking lifecycle segment power. Queries return 'starting' status. |
| RUNNING | Stable | Actively collecting data and (for bandits) adapting policy. |
| CONCLUDING | Transitional (NEW) | Running final analysis, creating policy snapshots, computing surrogate projections, generating IPW estimates. Prevents premature result queries. |
| CONCLUDED | Stable | Analysis complete, results available, experiment no longer collecting data. |
| ARCHIVED | Stable | Experiment and results retained for historical reference. |


During STARTING, the Assignment Service does not yet serve this experiment's assignments — preventing partial rollout before validation completes. During CONCLUDING, the UI shows a progress indicator rather than stale or incomplete results. This eliminates the race condition where a PM queries results while the final analysis is still computing.

Figure 4: Experiment Lifecycle State Machine
See accompanying file: state_machine.mermaid
Full state machine with stable states (DRAFT, RUNNING, CONCLUDED, ARCHIVED) and transitional states (STARTING, CONCLUDING). STARTING sub-states: ValidateConfig, WarmBandit, ConfirmMetrics, CheckSegmentPower. CONCLUDING sub-states: FinalMetrics, FinalAnalysis, SurrogateProjections, PolicySnapshot, GenerateReport. RUNNING includes auto-pause on guardrail breach. Annotations show module ownership: M5 orchestrates transitions, M1 blocks during STARTING, M4a runs analysis during CONCLUDING, M6 shows progress indicators.


## 2.6 SDK Provider Abstraction (from GrowthBook)

GrowthBook users wrap the SDK in a provider abstraction layer so the underlying platform can be swapped without changing call sites. We build this directly into our SDK architecture:

// SDK Provider Interface (TypeScript example)
interface ExperimentProvider {
getAssignment(userId: string, experimentId: string): Assignment;
getAssignments(userId: string, attributes: Record<string, string>): Assignment[];
logExposure(exposure: ExposureEvent): void;
}

// Concrete providers
class RemoteProvider implements ExperimentProvider { /* gRPC to Assignment Service */ }
class LocalProvider implements ExperimentProvider  { /* WASM hash, cached config */ }
class MockProvider implements ExperimentProvider   { /* deterministic, for unit tests */ }

// Fallback chain: Remote -> Local cache -> Static defaults
class ResilientProvider implements ExperimentProvider {
constructor(private providers: ExperimentProvider[]) {}
getAssignment(userId, experimentId) {
for (const p of this.providers) {
try { return p.getAssignment(userId, experimentId); }
catch { continue; }
}
return DEFAULT_ASSIGNMENT;
}
}

The ResilientProvider is critical for mobile SDKs where network unreliability is common. The LocalProvider uses the WASM-compiled hash library with a cached config snapshot, producing deterministic assignments offline. The MockProvider enables unit testing of product code without network dependencies.

Figure 5: SDK Provider Fallback Chain
See accompanying file: sdk_provider.mermaid
Application code calls getAssignment() on a ResilientProvider that tries providers in sequence: (1) RemoteProvider via gRPC to M1 Assignment Service, (2) LocalProvider with WASM/UniFFI hash and cached config, (3) Static defaults. MockProvider is injected for unit testing. Platform-specific builds: Web uses fetch + WASM, iOS uses gRPC-Swift + UniFFI, Android uses gRPC-Kotlin + UniFFI, Server uses gRPC + CGo/PyO3.


## 2.7 SQL Transparency & Notebook Export (from GrowthBook)

GrowthBook exposes all SQL in analyses and lets users export to Jupyter notebooks. We adopt this for the Metric Computation Engine:
Every metric computation logs the Spark SQL it executed to a query_log table in PostgreSQL, keyed by (experiment_id, metric_id, computation_timestamp).
The UI exposes a 'View SQL' button on every metric result, showing the exact query used to compute it.
An 'Export to Notebook' button generates a Databricks/Jupyter notebook (.ipynb) with the SQL queries, data loading code, and the statistical analysis pipeline — fully reproducible outside the platform.
This builds trust with data scientists who need to validate platform results against their own analyses, and provides an escape hatch for custom analyses the platform does not support.


## 2.8 Group Sequential Tests (from Spotify Confidence)

Spotify's Confidence platform offers both always-valid inference (like our existing mSPRT) and group sequential tests (GSTs) for teams that can pre-commit to a fixed analysis schedule. GSTs are more powerful than mSPRT when the number of looks is known in advance:
mSPRT (retained): Always-valid p-values with arbitrary peeking. Lower power but maximum flexibility. Best for: teams that check dashboards continuously.
Group Sequential Tests (NEW): Pre-specified analysis schedule (e.g., 'look at results every Monday for 4 weeks'). Spending functions (O'Brien-Fleming, Pocock) allocate alpha across looks. Higher power than mSPRT at the same sample size. Best for: planned weekly reviews with a known experiment duration.
The Analysis Engine supports both methods. The experiment configuration specifies which sequential method to use and, for GSTs, the number of planned looks and spending function.


## 2.9 Automated Bucket Reuse (from Spotify Confidence)

Spotify builds bucket reuse directly into their experimentation platform so teams can run many experiments autonomously without coordinating test groups. We add automatic hash-space recycling:
When an experiment transitions to CONCLUDED, its hash-space allocation within its layer is automatically returned to the layer's available pool.
A configurable cooldown period (default 24 hours) prevents immediate reuse, allowing late-arriving exposures to drain.
The Assignment Service's config snapshot is updated to reflect the freed allocation, and new experiments can claim the recycled space.
This prevents traffic exhaustion as experiment volume grows — without bucket reuse, a platform running 100+ experiments per quarter would quickly exhaust its hash space.


## 2.10 Guardrails Default to Auto-Pause (from Spotify)

Spotify developers roll back approximately 42% of experiments to prevent business metric regressions, indicating aggressive guardrail enforcement. We adopt auto-pause as the default:
When a guardrail metric breaches its configured threshold, the experiment is automatically paused (traffic allocation set to 0%, no new assignments).
The experiment owner receives an immediate alert (Slack + PagerDuty for critical guardrails) with the breaching metric, current value, and threshold.
To continue an experiment despite a guardrail breach, the owner must explicitly set guardrail_action: ALERT_ONLY on the experiment configuration — an audited, non-default action.
This safe-by-default behavior protects the platform's credibility: PMs trust that shipping a treatment that passed guardrails actually means something.


# 3. Proto Schema Architecture

All modules share a Protobuf schema layer managed by the buf toolchain. The schema includes SVOD-specific message types for interleaving, surrogate metrics, QoE events, lifecycle segmentation, and the experiment state machine with transitional states.


## 3.1 Repository Structure

proto/experimentation/
common/v1/
experiment.proto        # Experiment, Variant, ExperimentState (with STARTING/CONCLUDING)
metric.proto            # MetricDefinition (surrogate, QoE, lifecycle support)
event.proto             # Exposure, Metric, QoE events
layer.proto             # Layer, Allocation (with bucket reuse cooldown)
targeting.proto         # TargetingRule predicate tree
bandit.proto            # BanditConfig, RewardEvent, PolicySnapshot
interleaving.proto      # InterleavingConfig, InterleavingScore, CreditAssignment
surrogate.proto         # SurrogateModelConfig, SurrogateProjection
lifecycle.proto         # LifecycleSegment, LifecycleStratificationConfig
qoe.proto               # QoEEvent, PlaybackMetrics
assignment/v1/            # AssignmentService RPCs
pipeline/v1/              # EventIngestionService RPCs
metrics/v1/               # MetricComputationService RPCs
analysis/v1/              # AnalysisService RPCs (mSPRT + GST)
bandit/v1/                # BanditPolicyService RPCs (includes cold-start)
management/v1/            # ExperimentManagementService RPCs
flags/v1/                 # FeatureFlagService RPCs


## 3.2 Experiment State Machine (Updated)

enum ExperimentState {
EXPERIMENT_STATE_UNSPECIFIED = 0;
EXPERIMENT_STATE_DRAFT = 1;
EXPERIMENT_STATE_STARTING = 2;      // NEW: transitional (validating, warming up)
EXPERIMENT_STATE_RUNNING = 3;
EXPERIMENT_STATE_CONCLUDING = 4;    // NEW: transitional (running final analysis)
EXPERIMENT_STATE_CONCLUDED = 5;
EXPERIMENT_STATE_ARCHIVED = 6;
}

// Layer allocation with bucket reuse support
message LayerAllocation {
string experiment_id = 1;
uint32 start_bucket = 2;
uint32 end_bucket = 3;
google.protobuf.Timestamp released_at = 4;   // when allocation was freed
google.protobuf.Timestamp reusable_after = 5; // cooldown expiry
}


## 3.3 SVOD-Specific Protos: interleaving, surrogate, lifecycle, qoe

// interleaving.proto
enum InterleavingMethod {
INTERLEAVING_METHOD_TEAM_DRAFT = 0;     // Alternating picks; fair coin for first pick
INTERLEAVING_METHOD_OPTIMIZED = 1;       // Radlinski & Craswell (2010)
INTERLEAVING_METHOD_MULTILEAVE = 2;      // Schuth et al. (2015); 3+ algorithms
}
enum CreditAssignment {
CREDIT_ASSIGNMENT_BINARY_WIN = 0;        // 1 if user engaged with algorithm's item, 0 otherwise
CREDIT_ASSIGNMENT_PROPORTIONAL = 1;      // Credit proportional to engagement depth
CREDIT_ASSIGNMENT_WEIGHTED = 2;          // Position-weighted credit (higher rank = more credit)
}
message InterleavingConfig {
InterleavingMethod method = 1;
repeated string algorithm_ids = 2;       // Minimum 2 algorithms required
CreditAssignment credit_assignment = 3;
string credit_metric_event = 4;          // e.g., 'play_start', 'watch_30s'
}

// surrogate.proto
enum SurrogateModelType {
SURROGATE_MODEL_LINEAR = 0;
SURROGATE_MODEL_GRADIENT_BOOSTED = 1;
SURROGATE_MODEL_NEURAL = 2;
}
message SurrogateModelConfig {
string model_id = 1;
string target_metric_id = 2;             // e.g., '90_day_churn_rate'
repeated string input_metric_ids = 3;    // e.g., ['7d_watch_time', '7d_session_freq']
int32 observation_window_days = 4;       // Short-term window (e.g., 7)
int32 prediction_horizon_days = 5;       // Long-term target (e.g., 90)
SurrogateModelType model_type = 6;
double calibration_r_squared = 7;        // Updated by periodic calibration job
}
message SurrogateProjection {
string experiment_id = 1;
string variant_id = 2;
double projected_effect = 3;
double projection_ci_lower = 4;
double projection_ci_upper = 5;
double calibration_r_squared = 6;        // Snapshot at computation time
}

// lifecycle.proto
enum LifecycleSegment {
LIFECYCLE_SEGMENT_TRIAL = 0;             // Free trial period
LIFECYCLE_SEGMENT_NEW = 1;               // Subscribed < 30 days
LIFECYCLE_SEGMENT_ESTABLISHED = 2;       // 30-180 days
LIFECYCLE_SEGMENT_MATURE = 3;            // > 180 days
LIFECYCLE_SEGMENT_AT_RISK = 4;           // Declining engagement signal
LIFECYCLE_SEGMENT_WINBACK = 5;           // Previously churned, resubscribed
}
message LifecycleStratificationConfig {
bool enabled = 1;
repeated LifecycleSegment segments = 2;  // Segments to analyze
int32 min_users_per_segment = 3;         // Power threshold (default 1000)
}

// qoe.proto
message PlaybackMetrics {
int64 time_to_first_frame_ms = 1;
int32 rebuffer_count = 2;
double rebuffer_ratio = 3;               // rebuffer_duration / playback_duration
int32 avg_bitrate_kbps = 4;
int32 resolution_switches = 5;
int32 peak_resolution_height = 6;        // e.g., 1080, 2160
double startup_failure_rate = 7;
int64 playback_duration_ms = 8;
}
message QoEEvent {
string session_id = 1;
string content_id = 2;
string user_id = 3;
PlaybackMetrics metrics = 4;
google.protobuf.Timestamp timestamp = 5;
}


## 3.4 New: Sequential Testing Config

enum SequentialMethod {
SEQUENTIAL_METHOD_UNSPECIFIED = 0;
SEQUENTIAL_METHOD_MSPRT = 1;         // Always-valid, arbitrary peeking
SEQUENTIAL_METHOD_GST_OBF = 2;       // Group sequential, O'Brien-Fleming spending
SEQUENTIAL_METHOD_GST_POCOCK = 3;    // Group sequential, Pocock spending
}

message SequentialTestConfig {
SequentialMethod method = 1;
int32 planned_looks = 2;             // For GST: number of pre-specified analyses
double overall_alpha = 3;            // Total Type I error budget (default 0.05)
}


## 3.5 New: Guardrail Action Config

enum GuardrailAction {
GUARDRAIL_ACTION_UNSPECIFIED = 0;
GUARDRAIL_ACTION_AUTO_PAUSE = 1;    // DEFAULT: pause experiment on breach
GUARDRAIL_ACTION_ALERT_ONLY = 2;    // Alert but continue (requires explicit opt-in)
}

// On Experiment message:
GuardrailAction guardrail_action = 26; // defaults to AUTO_PAUSE


# 4. Module 1: Assignment Service (Agent-1, Rust)


## 4.1 Purpose & Crash-Only Design

The Assignment Service is entirely stateless: it fetches experiment configuration from the Management Service via streaming RPC, caches it in-process, and computes assignments via deterministic hashing. On crash, a restarted instance re-fetches config and produces identical assignments immediately. There is no warm-up state, no local persistence, and no separate shutdown path.
Crate dependencies: experimentation-assignment depends on experimentation-hash, experimentation-proto, experimentation-interleaving, experimentation-core.


## 4.2 Assignment Modes


| Mode | Experiment Types | Description |
| --- | --- | --- |
| Static User-Level | A/B, Multivariate, Holdout, Cumulative Holdout | MurmurHash3 bucketing. Deterministic, no external state. |
| Session-Level | Session-Level | hash(session_id + experiment_id + salt). Same user varies across sessions. |
| Interleaving | Interleaving | Constructs merged list from 2+ algorithm outputs via Team Draft or Optimized. |
| Bandit | MAB, Contextual Bandit, Cold-Start | Calls Bandit Policy Service via low-latency gRPC. |


## 4.3 Bucket Reuse

When the config snapshot indicates an experiment has transitioned to CONCLUDED and the cooldown period has elapsed, the Assignment Service stops serving that experiment's assignments and the hash-space allocation becomes available for new experiments. The cooldown (default 24 hours) allows late-arriving exposure events to be associated with the correct experiment before the allocation is recycled.


## 4.4 SDK Architecture (Provider-Based)

Server SDKs (Go/Python): RemoteProvider calls Assignment Service via gRPC.
Web SDK: ResilientProvider with RemoteProvider (fetch API) falling back to LocalProvider (WASM hash + cached config from localStorage).
Mobile SDKs (iOS/Android): ResilientProvider with RemoteProvider falling back to LocalProvider (UniFFI hash + cached config). MockProvider for unit tests.
All providers implement the same ExperimentProvider interface. Product code is provider-agnostic.


## 4.5 Acceptance Criteria

Cross-platform determinism for all 10,000 test vectors across Rust native, WASM, UniFFI, CGo, Python.
Static assignment p99 < 5ms at 50K rps.
Interleaving list construction p99 < 8ms for 50-item lists with 2 algorithms.
Crash recovery: restarted instance serves identical assignments within 2 seconds (config re-fetch).
Bucket reuse: concluded experiment allocation is recycled after cooldown.
SDK fallback: LocalProvider serves cached assignments when RemoteProvider is unreachable.


# 5. Module 2: Event Pipeline (Agent-2, Rust + Go)


Figure 6: End-to-End Data Flow Pipeline
See accompanying file: data_flow.mermaid
Full data flow from event sources (App, CDN, Rec Service) through M2 Event Ingestion (Rust: validate, dedup, Kafka publish) to Kafka topics (exposures, metric_events, reward_events, qoe_events, guardrail_alerts), then to M3 Metric Engine (Go + Spark) for metric computation, Delta Lake for storage, M4a/M4b (Rust) for statistical analysis and bandit policy, PostgreSQL for results, and finally M6 UI (TypeScript) for dashboards. Guardrail alerts flow to M5 for auto-pause.


## 5.1 Crash-Only Ingestion (Rust)

The Rust ingestion service is stateless: it validates events, deduplicates via Bloom filter (rebuilt from scratch on startup), and publishes to Kafka. At-least-once delivery; downstream consumers handle idempotency. On crash, the service restarts with an empty Bloom filter — a brief window of duplicate events that downstream dedup absorbs.


## 5.2 Event Types & Kafka Topics


| Topic | Schema | Key Consumers |
| --- | --- | --- |
| exposures | ExposureEvent | Metric Engine, Monitoring |
| metric_events | MetricEvent | Metric Engine, Monitoring |
| reward_events | RewardEvent | Bandit Policy Service, Metric Engine |
| qoe_events | QoEEvent | Metric Engine, QoE Dashboard |
| guardrail_alerts | GuardrailAlert | Management Service (auto-pause trigger), UI |


Interleaving engagement events include source_algorithm_id provenance for credit assignment. QoE events carry PlaybackMetrics (TTFF, rebuffer ratio, bitrate, resolution switches).


## 5.3 SQL Logging for Transparency

The Go orchestration layer logs all Spark/Flink SQL submitted for metric computation to a query_log table. Each row includes experiment_id, metric_id, the full SQL text, computation_timestamp, and row_count. This feeds the UI's 'View SQL' and 'Export to Notebook' features.


## 5.4 Acceptance Criteria

Rust ingestion service sustains 100K events/sec at p99 < 20ms (measured via criterion benchmark with synthetic ExposureEvent, MetricEvent, QoEEvent mix).
All event types (ExposureEvent, MetricEvent, RewardEvent, QoEEvent) land in Delta Lake within 5 minutes and Redis within 30 seconds of Kafka publish.
Crash recovery: restarted Rust ingestion process accepts and publishes events within 1 second of startup. Brief duplicate window (empty Bloom filter) verified to be absorbed by downstream M3 deduplication.
Go orchestration layer writes every Spark SQL query to PostgreSQL query_log table with experiment_id, metric_id, full SQL text, computation_timestamp, duration_ms, and row_count.


# 6. Module 3: Metric Computation Engine (Agent-3, Go)


## 6.1 Purpose

Transforms raw events into per-user metric summaries. Includes interleaving scoring, surrogate computation, lifecycle-segmented metrics, content consumption analysis, session-level aggregation, and QoE metrics. All Spark SQL is logged for transparency.


## 6.2 SVOD-Specific Computations

Interleaving Scoring (Spark SQL): Join exposure events (containing source_algorithm_id provenance from M1) with engagement events; apply credit assignment method (Binary Win, Proportional, or Weighted as configured in InterleavingConfig); aggregate to InterleavingScore proto per user. Output consumed by M4a for significance testing.
Surrogate Metrics (Spark SQL + MLflow): Compute short-term input metrics per variant via Spark, load trained surrogate model from MLflow registry, apply model to produce SurrogateProjection proto. Periodic Go-scheduled calibration job retrains model on historical experiments where long-term outcomes have been observed.
Lifecycle Segmentation (Spark SQL + Redis): At exposure time, classify each user into LifecycleSegment by joining with subscription attributes in Redis feature store. Compute all metrics separately per segment, producing keyed rows: (experiment_id, user_id, metric_id, lifecycle_segment). Output consumed by M4a for stratified analysis.
Content Consumption Analysis (Spark SQL): Compute per-variant title-level consumption distributions. Calculate Jaccard similarity of top-100 titles, Gini coefficient of watch-time distribution, and fraction of catalog receiving views. Output consumed by M4a for Jensen-Shannon divergence interference testing.
Session-Level Aggregation (Spark SQL): Aggregate metrics at (session_id, variant_id, metric_id) granularity, preserving user_id linkage column. Output consumed by M4a for clustered standard error computation.
QoE Metrics (Spark SQL on qoe_events topic): Aggregate PlaybackMetrics fields into per-user means: TTFF, rebuffer rate, rebuffer ratio, average bitrate, resolution stability index, startup failure rate. Compute QoE-engagement Pearson correlation.
Notebook Export (Go): Generate Databricks/Jupyter .ipynb files containing the exact Spark SQL queries from query_log, Python data loading boilerplate, and analysis code stubs. Served via M6 UI's Export to Notebook button.


## 6.3 Acceptance Criteria

Standard metric Spark jobs complete within 2 hours of daily event arrival; guardrail metric jobs complete within 30 minutes of hourly trigger.
Interleaving scores correctly attribute engagement to source algorithms: validated by synthetic test where Algorithm A items receive 100% engagement and Algorithm B items receive 0%, producing Algorithm A win rate of 1.0.
Surrogate projections computed and written to SurrogateProjection table within 1 hour of short-term metric Spark job completion.
All Spark SQL logged to PostgreSQL query_log table with experiment_id, metric_id, full SQL text, and row_count. No query executes without a log entry.
Export to Notebook produces .ipynb files that execute without modification in Databricks Runtime 14+ and JupyterLab 4+.


# 7. Module 4a: Statistical Analysis Engine (Agent-4, Rust)


## 7.1 Fail-Fast Implementation

The Analysis Engine panics on NaN, Infinity, negative durations, and arithmetic overflow. Property-based tests via proptest validate invariants in CI: CUPED never increases variance, bootstrap CI coverage is between 93-97% on synthetic datasets, sign test p-values match scipy reference implementation. These invariants are written in plain language first, then codified as executable checks.
Crate: experimentation-stats. Features: simd (enables SIMD intrinsics for bootstrap), python (PyO3 bindings for notebook use).


## 7.2 Sequential Testing (Updated: mSPRT + GST)


| Method | When to Use | Power | Flexibility |
| --- | --- | --- | --- |
| mSPRT (always-valid) | Continuous monitoring, no fixed schedule | Lower (pays for arbitrary peeking) | Look at results any time |
| GST O'Brien-Fleming | Fixed weekly reviews, conservative early stopping | Higher (alpha concentrated at end) | Must pre-commit to N looks |
| GST Pocock | Fixed reviews, equal stopping probability each look | Moderate | Must pre-commit to N looks |


The experiment configuration specifies sequential_test_config with the method and (for GST) planned_looks. The Analysis Engine computes spending-function-adjusted boundaries at each look and reports whether the effect has crossed the boundary. For GST, it also reports the remaining alpha budget and projected power at future looks.


## 7.3 SVOD-Specific Analysis Methods

Interleaving Significance (input: InterleavingScore from M3): Sign test (tests whether fraction of users favoring Algorithm A differs from 0.5) and Bradley-Terry model (estimates relative algorithm strength from pairwise win/loss data). Per-position analysis for slot-level effects.
Novelty Detection (input: daily treatment effect time-series from M3): Levenberg-Marquardt fit of exponential decay model: effect(t) = steady_state + novelty_amplitude * exp(-t / decay_constant). Flags novelty if novelty_amplitude CI excludes zero AND decay_constant < 14 days. Reports both raw and projected steady-state effect.
Content Interference (input: title-level consumption distributions from M3): Jensen-Shannon divergence between treatment and control title-level watch-time distributions. Title-level spillover test for individual titles with anomalous treatment/control differences. Catalog coverage comparison.
Lifecycle Stratification (input: per-segment metric summaries from M3): Pre-specified subgroup analysis per LifecycleSegment with Benjamini-Hochberg FDR correction across segments and Cochran Q test for treatment effect heterogeneity.
Session-Level Clustering (input: session-level metric rows with user_id from M3): HC1 sandwich estimator for clustered standard errors on user_id. Cluster-robust bootstrap resamples entire user clusters. Reports both naive (unclustered) and clustered results for comparison.
Surrogate-Augmented Reporting (input: SurrogateProjection from M3): Displays observed short-term treatment effects alongside surrogate-projected long-term effects. Confidence badge based on calibration_r_squared at projection time.


## 7.4 Core Statistical Methods (All Computed in Rust by M4a)

The following methods are implemented in the experimentation-stats Rust crate and executed by the M4a Analysis Engine binary. No statistical computation occurs in Go or TypeScript.
Fixed-horizon frequentist: Welch t-test (continuous metrics), z-test (proportions), delta method (ratio metrics), stratified bootstrap (10,000 resamples).
CUPED variance reduction: Pre-experiment covariate regression. Reduces required sample size by 30-50% for metrics with strong pre-period correlation.
Bayesian analysis: Beta-Binomial (proportions), Normal-Normal (continuous). Posterior computation with credible intervals and probability of beating control.
SRM detection: Chi-squared test on observed vs expected sample ratios. Flags experiments with p < 0.001.
Multiple comparison corrections: Benjamini-Hochberg FDR (across metrics and segments), Bonferroni (for primary metrics).
IPW-adjusted bandit analysis: Inverse-propensity-weighted treatment effect estimates from adaptive assignment data. Uses logged assignment probabilities from M4b.


## 7.5 Acceptance Criteria

proptest: CUPED never increases variance on 10,000 random datasets.
proptest: Bootstrap CI coverage 93-97% on synthetic data with known effect.
GST boundaries match validated R implementation (gsDesign package) to 4 decimal places.
Novelty steady-state projection within 15% of true value on synthetic data.
All analyses complete within 3 minutes for 1M users, 15 metrics, 6 segments.
Fail-fast: NaN input triggers immediate panic, not silent propagation.


# 8. Module 4b: Bandit Policy Service (Agent-4, Rust)


## 8.1 LMAX-Inspired Threading Model

All policy state mutations (posterior updates, model weight changes, snapshot writes) happen on a single dedicated thread. The gRPC server and Kafka consumer run on tokio async runtimes and communicate with the policy core via bounded channels. This eliminates locks on the Cholesky decomposition and posterior parameters.
Crate: experimentation-bandit depends on experimentation-core, experimentation-proto. Features: gpu (enables tch-rs for neural bandits).


## 8.2 Crash-Only Recovery

The policy core snapshots to RocksDB on every reward update as a side effect of normal operation. On crash, the recovery path is identical to startup: load the last snapshot from RocksDB, then replay reward events from the Kafka consumer group's committed offset. There is no separate 'save state on shutdown' code path.


## 8.3 Content Cold-Start Bandit

For new content launches: arms are recommendation placements, context is content metadata + user attributes, reward is play-through rate * completion rate. Runs for configurable duration (default 7 days), then exports learned audience affinity scores to the recommendation system.


## 8.4 Algorithms

Algorithms implemented in experimentation-bandit Rust crate: Thompson Sampling (Beta posterior for binary rewards, Normal posterior for continuous), Linear UCB (ridge regression with Sherman-Morrison updates), Thompson + Linear hybrid, Neural Contextual Bandit (tch-rs, 2-layer MLP with dropout). Exploration safeguards: minimum exploration floor (configurable, default 10% per arm), warmup period with uniform random assignment (configurable, default 1,000 observations), guardrail metric monitoring with auto-pause integration via M5, policy rollback to previous snapshot on degradation, and IPW probability logging to the exposures Kafka topic for downstream causal analysis by M4a.


## 8.5 Acceptance Criteria

SelectArm p99 < 15ms at 10K rps.
Single-threaded core: zero mutex contention under load (verified via tokio-console).
Crash recovery: policy state restored from RocksDB + Kafka replay within 10 seconds.
Cold-start bandit creates and manages experiment for new content automatically.


# 9. Module 5: Experiment Management Service (Agent-5, Go)


## 9.1 Lifecycle with Transitional States

The STARTING transitional state validates config, warms up bandit policy, confirms metric availability, and checks lifecycle segment power. The CONCLUDING transitional state runs final analysis, creates policy snapshots, computes surrogate projections, and generates IPW estimates. API queries during these states return appropriate status indicators.


## 9.2 Auto-Pause Guardrails

Guardrail breaches trigger AUTO_PAUSE by default: traffic allocation set to 0%, owner alerted via Slack + PagerDuty. To override, experiment must be explicitly configured with guardrail_action: ALERT_ONLY. This is an audited action logged in the experiment audit trail.


## 9.3 Experiment Type Support


| Type | STARTING Validation (M5 enforces) | CONCLUDING Behavior (M5 triggers) |
| --- | --- | --- |
| A/B, Multivariate | Variant count >= 2, primary metric defined, traffic allocation sums to 100% | M4a runs fixed-horizon or sequential analysis; M5 transitions to CONCLUDED on completion |
| INTERLEAVING | InterleavingConfig present, 2+ algorithm_ids, credit_metric_event maps to valid event type | M4a runs sign test + Bradley-Terry; M5 transitions to CONCLUDED on completion |
| SESSION_LEVEL | SessionConfig present, session_id_attribute resolves in event schema, min_sessions_per_user > 0 | M4a runs clustered analysis (naive + HC1); M5 transitions to CONCLUDED on completion |
| PLAYBACK_QOE | At least one QoE guardrail metric (TTFF, rebuffer, bitrate) defined in metric set | M4a runs QoE analysis cross-referenced with engagement; M5 transitions to CONCLUDED |
| CONTEXTUAL_BANDIT | BanditConfig present, reward_metric defined, context features resolve in feature store | M4a runs IPW-adjusted causal analysis using logged probabilities from M4b; M5 transitions |
| CUMULATIVE_HOLDOUT | Holdout percentage 1-5%, baseline algorithm version pinned, no conclusion_date set | No auto-conclusion; M4a produces periodic cumulative lift report on configurable schedule |


## 9.4 Surrogate & Bucket Reuse Management

Surrogate model RPCs: CreateSurrogateModel, ListSurrogateModels, GetSurrogateCalibration, TriggerSurrogateRecalibration. Bucket reuse: when experiment concludes, allocation marked with released_at timestamp; reusable_after = released_at + cooldown_duration. Management Service validates that new experiments do not claim allocations still in cooldown.


## 9.5 Acceptance Criteria

STARTING state blocks assignment serving until validation completes.
CONCLUDING state blocks result queries until analysis completes.
Auto-pause triggers within 60 seconds of guardrail breach detection on the guardrail_alerts Kafka topic.
Bucket reuse validation rejects new experiment allocation claims for slots still within cooldown period.
Surrogate model registration validates that all input_metric_ids exist in the experiment metric set before accepting.
Lifecycle stratification warns at experiment creation if any segment would have fewer than 1,000 users (underpowered).
Cumulative holdout experiments persist indefinitely with no auto-conclusion; periodic reporting runs on configurable schedule.
All experiment types enforce type-specific validation gates (see Section 9.3 table) before transitioning from DRAFT to STARTING.


# 10. Module 6: Decision Support UI (Agent-6, TypeScript)


## 10.1 SQL Transparency Features (from GrowthBook)

View SQL button on every metric result panel. Opens modal with syntax-highlighted SQL and copy-to-clipboard.
Export to Notebook button generates a Databricks/Jupyter .ipynb with queries, data loading, and analysis code.
Query history timeline showing when each metric was last computed and how long it took.


## 10.2 SVOD-Specific Views (All Rendered by M6 Frontend; Data from M3/M4a/M4b Backend)

Each view below is a frontend visualization rendered in the TypeScript UI. All underlying data is computed by the Rust or Go backend services indicated in parentheses. The UI performs no statistical computation.
Interleaving: Live win-rate chart, per-position heatmap, Bradley-Terry strength estimates. Data source: M4a Analysis Engine (Rust).
Surrogate Projections: Dual-panel observed vs projected with confidence badge (green/yellow/red by R-squared). Data source: M3 Metric Engine (Go) computes projections; M4a reports them.
Novelty Effect: Time-series with fitted decay curve, dual-value display, stability indicator. Data source: M4a Analysis Engine (Rust) fits exponential decay model.
Content Interference: Venn-style overlap, Lorenz curve / Gini comparison, alert banner. Data source: M3 computes consumption distributions; M4a runs JS divergence test.
Lifecycle Segments: Forest plot per segment with significance indicators, Cochran Q, power status. Data source: M3 computes per-segment metrics; M4a runs stratified analysis.
Session-Level: Naive vs clustered SEs side-by-side, design effect indicator. Data source: M4a Analysis Engine (Rust) computes HC1 sandwich estimator.
Playback QoE: Traffic-light dashboard, QoE-engagement correlation scatter. Data source: M3 Metric Engine (Go) aggregates QoE telemetry.
Cumulative Holdout: Long-running time-series, monthly trend breakdown, holdout health monitoring. Data source: M4a Analysis Engine (Rust) computes cumulative lift.
Cold-Start Bandit: Active bandits dashboard, post-launch affinity scores. Data source: M4b Bandit Policy Service (Rust) exports learned scores.


## 10.3 Experiment State Indicators

The UI reflects transitional states: STARTING shows a validation progress checklist (config valid, bandit warming, metrics available, segments powered). CONCLUDING shows an analysis progress indicator (computing metrics, running tests, generating projections). These prevent PMs from seeing stale or incomplete results.


## 10.4 Sequential Testing Views

For mSPRT: Confidence sequence plot with always-valid boundaries.
For GST: Spending function chart showing alpha allocated at each planned look. Boundary crossing indicator.
Both: Clear indication of whether the experiment can be stopped at the current look.


## 10.5 Acceptance Criteria

View SQL modal renders syntax-highlighted Spark SQL for any metric result within 500ms of click.
Export to Notebook produces valid .ipynb that executes without modification in Databricks and Jupyter.
STARTING state indicator shows validation checklist (config valid, bandit warming, metrics available, segments powered).
CONCLUDING state indicator shows analysis progress bar; result panels are disabled until CONCLUDED.
GST boundary plots match Analysis Engine (M4a) computations to 4 decimal places.
Interleaving monitoring dashboard shows live win rates updated within 60 seconds of new exposure data.
Surrogate projection panels display confidence badge color (green R-squared > 0.7, yellow 0.5-0.7, red < 0.5).
Novelty effect time-series renders fitted decay curve overlay with dual-value display (Current Effect vs Steady-State).
Content interference Venn and Lorenz charts render for experiments with 2+ variants.
Lifecycle forest plot renders per-segment treatment effects with significance indicators and Cochran Q p-value.
Cumulative holdout time-series renders over multi-month windows with monthly trend breakdown.
Cold-start bandit dashboard shows active bandits with real-time arm allocation proportions and reward rates.


# 11. Module 7: Feature Flag Service (Agent-7, Go)


The Feature Flag Service provides boolean, string, numeric, and JSON flag evaluation with percentage rollouts and targeting rules. PromoteToExperiment converts a flag into a fully-tracked experiment of any type (A/B, INTERLEAVING, SESSION_LEVEL, CONTEXTUAL_BANDIT, CUMULATIVE_HOLDOUT). Flag evaluation uses the experimentation-hash Rust library via CGo to guarantee deterministic bucketing identical to the Assignment Service (M1). The Flag Service itself performs no statistical computation — promotion delegates to M5 (Management) for lifecycle and M4a/M4b for analysis.


## 11.1 Acceptance Criteria

Boolean flags evaluate in under 1ms locally.
PromoteToExperiment works for all experiment types.
Hash evaluation via CGo produces identical results to native Rust.


# 12. Cross-Cutting Concerns


## 12.1 Observability

Go: OpenTelemetry Go SDK + connect-go interceptors for automatic RPC tracing and Prometheus metrics.
Rust: tracing crate + opentelemetry-rust + tonic interceptors. Spans include crate name for dependency-level visibility.
Cross-language W3C Trace Context propagation through Connect/gRPC metadata.
NEW: Crash recovery metrics — time to first successful assignment after restart, Kafka replay lag after bandit recovery.
NEW: Single-threaded core metrics — policy_channel depth, reward_channel depth, events processed per second, channel backpressure events.
NEW: Guardrail auto-pause dashboard — experiments paused, breach rates, override audit log.
Alerting: PagerDuty (critical guardrails, crash recovery), Slack (informational, experiment lifecycle transitions).


## 12.2 Infrastructure


| Component | Technology | Notes |
| --- | --- | --- |
| Go Services | connect-go on K8s | Management, Metrics, Flags, Orchestration |
| Rust Services | tonic + tonic-web on K8s | Assignment, Ingestion, Analysis, Bandit (shared Cargo workspace) |
| Cargo Workspace | Shared crates/ directory | core, hash, stats, bandit, interleaving, ingest, ffi, proto, 4 service binaries |
| Schema | Buf CLI + BSR | Shared across Go and Rust, includes SVOD-specific protos |
| Kafka | MSK/Confluent | exposures, metric_events, reward_events, qoe_events, guardrail_alerts |
| Lakehouse | Delta Lake on S3/GCS | QoE tables, surrogate artifacts, query_log |
| Surrogate Models | MLflow on S3 | Model registry, versioning, calibration tracking |
| Feature Store | Redis Cluster | User attributes, lifecycle segment, bandit context features |
| Database | PostgreSQL | Config, metrics, results, audit, surrogates, query_log |
| Policy Store | RocksDB (embedded) | Bandit policy snapshots — crash-only persistence model |


# 13. Implementation Roadmap


## 13.1 Phase 0: Schema & Toolchain (Week 1)

Proto repo with all SVOD-specific and sequential testing protos. buf.yaml configured with buf lint + buf breaking in CI. Rust Cargo workspace skeleton: all 13 crate stubs with Cargo.toml dependency graph, feature flag definitions (wasm, uniffi, ffi, python, gpu, simd), and a single workspace-level CI job running cargo clippy --all-features. Go module skeleton with connect-go interceptor templates. Hash test vector file (10,000 entries) generated from reference Python implementation. sccache configured for Rust CI builds.


## 13.2 Phase 1: Foundation (Weeks 2-7)


| Agent | Lang | Deliverable |
| --- | --- | --- |
| Agent-1 | Rust | M1 Assignment Service (crash-only): static + session-level hashing. SDK provider interface definition. |
| Agent-2 | Rust+Go | M2 Event Ingestion (Rust, crash-only): all event types incl. QoE. M2 Orchestration (Go): SQL query logging. |
| Agent-5 | Go | M5 Management Service: CRUD, lifecycle w/ STARTING/CONCLUDING states, auto-pause guardrails. |
| Agent-3 | Go | M3 Metric Engine: core SVOD + QoE metric Spark jobs, lifecycle segmentation, query_log writes. |

Phase 1 Gate: One A/B experiment runs end-to-end. M1 (Rust) assigns users deterministically. M2 (Rust) ingests exposure and metric events to Kafka. M3 (Go) computes per-user metrics via Spark. M5 (Go) manages lifecycle including STARTING validation and auto-pause on guardrail breach.


## 13.3 Phase 2: Analysis & UI (Weeks 6-11)


| Agent | Lang | Deliverable |
| --- | --- | --- |
| Agent-4 | Rust | M4a Analysis Engine: frequentist, CUPED, SRM, mSPRT + GST, clustered, novelty. proptest suite. |
| Agent-6 | TypeScript | M6 UI only: experiment creation wizard, monitoring dashboards, results pages, View SQL modal, Export to Notebook button, lifecycle state indicators. |
| Agent-1 | Rust | M1 Assignment Service: WASM/UniFFI SDK builds via feature flags. Interleaving list construction RPC. Bucket reuse logic. |
| Agent-3 | Go | M3 Metric Engine: full SVOD + e-commerce metric library, content consumption analysis Spark jobs, .ipynb generation pipeline. |

Phase 2 Gate: M4a (Rust) produces automated statistical analysis with mSPRT + GST. M6 (TypeScript) renders results dashboards with View SQL and Export to Notebook. M1 (Rust) produces WASM + UniFFI SDK builds. LocalProvider serves cached assignments offline.


## 13.4 Phase 3: SVOD-Native + Bandits (Weeks 10-17)


| Agent | Lang | Deliverable |
| --- | --- | --- |
| Agent-4 | Rust | M4b Bandit Policy Service (LMAX single-thread core): Thompson Sampling, LinUCB, cold-start. Crash-only RocksDB snapshots. |
| Agent-4 | Rust | M4a Analysis Engine: interleaving significance tests (sign test, Bradley-Terry). Content interference detection (JS divergence). |
| Agent-3 | Go | M3 Metric Engine: interleaving scoring Spark jobs. Surrogate model pipeline + calibration. Notebook templates for surrogate analysis. |
| Agent-5 | Go | M5 Management Service: interleaving + holdout experiment lifecycle. Surrogate model RPCs. Bucket reuse allocation management. |
| Agent-6 | TypeScript | M6 UI only: interleaving win-rate charts, surrogate projection panels, interference Venn/Lorenz visualizations, lifecycle forest plots, bandit arm allocation dashboards, GST boundary plots. No backend computation. |
| Agent-7 | Go | M7 Feature Flag Service: all RPCs including PromoteToExperiment for all experiment types. |

Phase 3 Gate: M4b (Rust) serves bandit arm selections at p99 < 15ms via LMAX single-threaded core with RocksDB crash recovery. M4a (Rust) produces interleaving significance tests and content interference reports. M3 (Go) computes interleaving scores and surrogate projections. M6 (TypeScript) renders all SVOD-specific visualization panels.


## 13.5 Phase 4: Advanced & Polish (Weeks 16-22)


| Agent | Lang | Deliverable |
| --- | --- | --- |
| Agent-4 | Rust | M4b Neural bandit (tch-rs). M4a Bayesian analysis, IPW-adjusted bandit estimates, surrogate-augmented reporting, quasi-experimental methods (diff-in-diff, synthetic control). |
| Agent-5 | Go | M5 Management Service: cumulative holdout experiment type. Content holdout design support. |
| Agent-6 | TypeScript | M6 UI only: cumulative holdout time-series charts, cold-start bandit dashboard, UI polish, accessibility audit, onboarding wizard. |
| All | Rust+Go | Load testing (Locust for Go, criterion for Rust). Chaos engineering: crash recovery drills. PGO for Rust binaries. sccache optimization. |

Phase 4 Gate: M4b neural bandit operational with GPU inference. M4a produces Bayesian posteriors and IPW-adjusted estimates. M5 manages cumulative holdouts with indefinite-duration reporting. Chaos engineering validates all Rust services recover from kill -9 under load within documented SLAs.


# 14. Agent Coordination Protocol


## 14.1 Integration via Protobuf

Proto repo as single source of truth. buf lint + buf breaking in CI. Go: buf generate. Rust: tonic-build in shared build.rs. TypeScript: @connectrpc/connect-web.


## 14.2 Shared Libraries (Updated)


| Library | Language | Owner | Purpose |
| --- | --- | --- | --- |
| experimentation-proto | Protobuf | All | All protos incl. SVOD-specific; published to BSR |
| experimentation-core | Rust | Tech Lead | Timestamps, config, errors, tracing — Cargo workspace foundation |
| experimentation-hash | Rust | Agent-1 | MurmurHash3 + WASM + UniFFI + CGo (via feature flags) |
| experimentation-stats | Rust | Agent-4 | Bootstrap, CUPED, mSPRT, GST, novelty, interference — reusable in notebooks |
| experimentation-bandit | Rust | Agent-4 | Thompson Sampling, LinUCB, Neural, cold-start algorithms |
| experimentation-interleaving | Rust | Agent-1 | Team Draft + Optimized Interleaving |
| experimentation-ffi | Rust | Agent-1 | cbindgen C headers for Go CGo bridge |
| experimentation-surrogate | Go + Python | Agent-3 | Go: orchestration and MLflow integration. Python: model training scripts only (scikit-learn, XGBoost). Not a service. |
| connect-interceptors | Go | Tech Lead | Auth, tracing, metrics for connect-go |
| tonic-interceptors | Rust | Tech Lead | Auth, tracing, metrics for tonic |


## 14.3 Testing Strategy (Updated)

Unit: >90% coverage. Go services (M3, M5, M7): testify. Rust services (M1, M2, M4a, M4b): built-in #[test]. TypeScript UI (M6): vitest + React Testing Library.
Property-based (Rust only, M4a): proptest invariants for CUPED variance reduction, bootstrap CI coverage, GST boundary computation, hash determinism.
Contract: buf breaking detects proto schema regressions. Protocol parity tests verify Go (M3/M5) and Rust (M1/M4a) produce identical responses on shared test fixtures.
Statistical (M4a): Synthetic A/B, interleaving, session-level, bandit datasets with known ground truth. Validate against scipy and R (gsDesign) reference implementations.
Surrogate (M3 + M4a): Backtest surrogate models on historical experiments where long-term outcomes have been observed. Projections must fall within 25% of actual.
Hash (M1): 10,000 determinism vectors verified across Rust native, WASM, UniFFI (Swift/Kotlin), CGo, Python — all must produce identical bucket assignments.
Crash recovery (M1, M2, M4b): Chaos engineering — kill -9 Rust services under load, measure time to first correct response post-restart. M1/M2 target < 2 seconds; M4b targets < 10 seconds.
Load: Go services profiled with Locust. Rust services benchmarked with criterion. M4b single-threaded core monitored via tokio-console for channel depth and backpressure.


# 15. Appendix


## 15.1 References

NautilusTrader (nautilustrader.io): Rust/Python trading platform. Crate layering, crash-only design, fail-fast, LMAX threading, assurance-driven engineering.
GrowthBook (growthbook.io): Open-source experimentation. Warehouse-native, SDK abstraction, SQL transparency.
Spotify Confidence (confidence.spotify.com): Sequential testing (mSPRT + GST), bucket reuse, guardrail-driven rollbacks.
Chapelle et al. (2012): Large-scale Validation and Analysis of Interleaved Search Evaluation.
Radlinski & Craswell (2010): Optimized Interleaving for Online Retrieval Evaluation.
Schuth et al. (2015): Multileave Comparisons for Recommendation Systems.
Athey & Imbens (2016): Surrogate metrics and surrogacy in experimentation.
Li et al. (2010): LinUCB. Agrawal & Goyal (2013): Thompson Sampling.
Tang et al. (2010): Google Overlapping Experiment Infrastructure.
Martin Fowler: LMAX Architecture (martinfowler.com/articles/lmax.html).
Candea & Fox (2003): Crash-Only Software (USENIX HotOS).
ConnectRPC, tonic, Buf, UniFFI, wasm-bindgen. Netflix blog, Microsoft ExP.


## 15.2 Glossary


| Term | Definition |
| --- | --- |
| Crash-Only Design | Recovery and startup share one code path. No separate graceful-shutdown persistence. |
| Fail-Fast | Panic on invalid data (NaN, overflow) rather than silent propagation. |
| LMAX Disruptor | Single-threaded event loop for core state; async I/O on separate threads via channels. |
| Provider Abstraction | SDK interface with pluggable backends (Remote, Local, Mock) and fallback chain. |
| Bucket Reuse | Recycling hash-space from concluded experiments after cooldown. |
| Auto-Pause Guardrail | Default behavior: experiment paused on guardrail breach. Override requires explicit config. |
| Group Sequential Test | Pre-specified analysis schedule with alpha spending. More powerful than mSPRT when looks are fixed. |
| proptest | Rust property-based testing. Generates random inputs to verify invariants hold universally. |
| Interleaving | Mixing items from 2+ algorithms in one list; 10-100x more sensitive than A/B. |
| Team Draft | Interleaving method: algorithms alternate picks like sports captains. |
| Surrogate Metric | Short-term metric validated to predict long-term outcomes. |
| Novelty Effect | Transient engagement spike that decays to steady-state. |
| Content Interference | Spillover from treatment redirecting consumption to specific titles. |
| Lifecycle Segment | User class by subscription maturity (trial to winback). |
| Clustered SE | Variance adjustment for correlated sessions within users. |
| QoE | Quality of Experience: TTFF, rebuffer, bitrate metrics. |
| CUPED | Variance reduction via pre-experiment covariates. |
| mSPRT | Sequential testing with always-valid p-values. |
