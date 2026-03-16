// =============================================================================
// k6 Load Test — M2 Event Pipeline: p99 < 10ms at configurable rps
// =============================================================================
// Validates SLA: IngestExposure/IngestMetricEvent p99 < 10ms under sustained load.
// VU counts scale dynamically with TARGET_RPS (baseline: 10K rps).
//
// Uses k6's native gRPC module against the tonic gRPC server.
// Exercises all 4 event types:
//   - IngestExposure (40% of traffic)
//   - IngestMetricEvent (30% of traffic)
//   - IngestRewardEvent (15% of traffic)
//   - IngestQoEEvent (15% of traffic)
//
// Usage:
//   k6 run scripts/loadtest_pipeline.js
//   PIPELINE_ADDR=localhost:50052 k6 run scripts/loadtest_pipeline.js
//   k6 run --env TARGET_RPS=50000 scripts/loadtest_pipeline.js
//
// Full automated run:
//   bash scripts/loadtest_pipeline.sh
//   TARGET_RPS=50000 bash scripts/loadtest_pipeline.sh
// =============================================================================

import grpc from "k6/net/grpc";
import { check } from "k6";
import { Rate, Trend, Counter } from "k6/metrics";

// ---------------------------------------------------------------------------
// Custom Metrics
// ---------------------------------------------------------------------------

const exposureLatency = new Trend("m2_exposure_latency", true);
const metricLatency = new Trend("m2_metric_latency", true);
const rewardLatency = new Trend("m2_reward_latency", true);
const qoeLatency = new Trend("m2_qoe_latency", true);
const errorRate = new Rate("m2_error_rate");
const totalCount = new Counter("m2_total");

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

const ADDR = __ENV.PIPELINE_ADDR || "localhost:50052";
const TARGET_RPS = parseInt(__ENV.TARGET_RPS || "10000");
const DURATION = __ENV.DURATION || "60s";
const RAMP_DURATION = __ENV.RAMP_DURATION || "10s";

// Scale VU counts proportionally to TARGET_RPS (baseline: 10K rps)
const VU_SCALE = Math.max(1, TARGET_RPS / 10000);

const SERVICE = "experimentation.pipeline.v1.EventIngestionService";

const client = new grpc.Client();
client.load(["proto"], "experimentation/pipeline/v1/pipeline_service.proto");

// ---------------------------------------------------------------------------
// Scenarios
// ---------------------------------------------------------------------------

export const options = {
  scenarios: {
    // IngestExposure (40% of traffic)
    ingest_exposure: {
      executor: "constant-arrival-rate",
      rate: Math.floor(TARGET_RPS * 0.40),
      timeUnit: "1s",
      duration: DURATION,
      preAllocatedVUs: Math.ceil(50 * VU_SCALE),
      maxVUs: Math.ceil(250 * VU_SCALE),
      exec: "ingestExposure",
      gracefulStop: "5s",
    },

    // IngestMetricEvent (30% of traffic)
    ingest_metric: {
      executor: "constant-arrival-rate",
      rate: Math.floor(TARGET_RPS * 0.30),
      timeUnit: "1s",
      duration: DURATION,
      preAllocatedVUs: Math.ceil(40 * VU_SCALE),
      maxVUs: Math.ceil(200 * VU_SCALE),
      exec: "ingestMetricEvent",
      gracefulStop: "5s",
    },

    // IngestRewardEvent (15% of traffic)
    ingest_reward: {
      executor: "constant-arrival-rate",
      rate: Math.floor(TARGET_RPS * 0.15),
      timeUnit: "1s",
      duration: DURATION,
      preAllocatedVUs: Math.ceil(20 * VU_SCALE),
      maxVUs: Math.ceil(100 * VU_SCALE),
      exec: "ingestRewardEvent",
      gracefulStop: "5s",
    },

    // IngestQoEEvent (15% of traffic)
    ingest_qoe: {
      executor: "constant-arrival-rate",
      rate: Math.floor(TARGET_RPS * 0.15),
      timeUnit: "1s",
      duration: DURATION,
      preAllocatedVUs: Math.ceil(20 * VU_SCALE),
      maxVUs: Math.ceil(100 * VU_SCALE),
      exec: "ingestQoEEvent",
      gracefulStop: "5s",
    },
  },

  thresholds: {
    // M2 SLA: p99 < 10ms for exposure and metric ingestion
    "m2_exposure_latency": ["p(99) < 10"],
    "m2_metric_latency": ["p(99) < 10"],
    "m2_error_rate": ["rate < 0.001"],

    // Overall gRPC
    "grpc_req_duration": ["p(99) < 15"],
  },
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

let eventCounter = 0;

function uniqueEventId(prefix) {
  eventCounter++;
  return `${prefix}-${eventCounter}-${Date.now()}`;
}

function randomUserId() {
  return `user-${Math.floor(Math.random() * 1_000_000)}`;
}

function randomSessionId() {
  return `session-${Math.floor(Math.random() * 100_000)}`;
}

function nowTimestamp() {
  const secs = Math.floor(Date.now() / 1000);
  return { seconds: secs.toString(), nanos: 0 };
}

// Experiment pools
const EXPERIMENTS = [
  "exp_dev_001",
  "exp_dev_002",
  "exp_dev_003",
  "exp_dev_005",
  "exp_dev_006",
];
const VARIANTS = ["control", "treatment_a", "treatment_b"];
const PLATFORMS = ["web", "ios", "android", "tv"];
const EVENT_TYPES = [
  "play_start",
  "watch_complete",
  "search",
  "add_to_list",
  "browse",
];

function randomPick(arr) {
  return arr[Math.floor(Math.random() * arr.length)];
}

// ---------------------------------------------------------------------------
// Scenario: IngestExposure
// ---------------------------------------------------------------------------

export function ingestExposure() {
  client.connect(ADDR, { plaintext: true, timeout: "5s" });

  const req = {
    event: {
      event_id: uniqueEventId("exp"),
      experiment_id: randomPick(EXPERIMENTS),
      user_id: randomUserId(),
      variant_id: randomPick(VARIANTS),
      timestamp: nowTimestamp(),
      platform: randomPick(PLATFORMS),
    },
  };

  const start = Date.now();
  const res = client.invoke(`${SERVICE}/IngestExposure`, req);
  const elapsed = Date.now() - start;

  exposureLatency.add(elapsed);
  totalCount.add(1);
  errorRate.add(res.status !== grpc.StatusOK);

  check(res, {
    "IngestExposure: status OK": (r) => r.status === grpc.StatusOK,
    "IngestExposure: accepted": (r) =>
      r.status === grpc.StatusOK && r.message && r.message.accepted,
  });

  client.close();
}

// ---------------------------------------------------------------------------
// Scenario: IngestMetricEvent
// ---------------------------------------------------------------------------

export function ingestMetricEvent() {
  client.connect(ADDR, { plaintext: true, timeout: "5s" });

  const req = {
    event: {
      event_id: uniqueEventId("met"),
      user_id: randomUserId(),
      event_type: randomPick(EVENT_TYPES),
      value: Math.random() * 3600,
      content_id: `content-${Math.floor(Math.random() * 1000)}`,
      session_id: randomSessionId(),
      timestamp: nowTimestamp(),
    },
  };

  const start = Date.now();
  const res = client.invoke(`${SERVICE}/IngestMetricEvent`, req);
  const elapsed = Date.now() - start;

  metricLatency.add(elapsed);
  totalCount.add(1);
  errorRate.add(res.status !== grpc.StatusOK);

  check(res, {
    "IngestMetricEvent: status OK": (r) => r.status === grpc.StatusOK,
    "IngestMetricEvent: accepted": (r) =>
      r.status === grpc.StatusOK && r.message && r.message.accepted,
  });

  client.close();
}

// ---------------------------------------------------------------------------
// Scenario: IngestRewardEvent
// ---------------------------------------------------------------------------

export function ingestRewardEvent() {
  client.connect(ADDR, { plaintext: true, timeout: "5s" });

  const req = {
    event: {
      event_id: uniqueEventId("rew"),
      experiment_id: randomPick(EXPERIMENTS),
      user_id: randomUserId(),
      arm_id: `arm-${Math.floor(Math.random() * 4)}`,
      reward: Math.random(),
      timestamp: nowTimestamp(),
    },
  };

  const start = Date.now();
  const res = client.invoke(`${SERVICE}/IngestRewardEvent`, req);
  const elapsed = Date.now() - start;

  rewardLatency.add(elapsed);
  totalCount.add(1);
  errorRate.add(res.status !== grpc.StatusOK);

  check(res, {
    "IngestRewardEvent: status OK": (r) => r.status === grpc.StatusOK,
    "IngestRewardEvent: accepted": (r) =>
      r.status === grpc.StatusOK && r.message && r.message.accepted,
  });

  client.close();
}

// ---------------------------------------------------------------------------
// Scenario: IngestQoEEvent
// ---------------------------------------------------------------------------

export function ingestQoEEvent() {
  client.connect(ADDR, { plaintext: true, timeout: "5s" });

  const req = {
    event: {
      event_id: uniqueEventId("qoe"),
      session_id: randomSessionId(),
      content_id: `content-${Math.floor(Math.random() * 1000)}`,
      user_id: randomUserId(),
      metrics: {
        time_to_first_frame_ms: Math.floor(Math.random() * 5000).toString(),
        rebuffer_count: Math.floor(Math.random() * 10),
        rebuffer_ratio: Math.random() * 0.1,
        avg_bitrate_kbps: 2000 + Math.floor(Math.random() * 8000),
        resolution_switches: Math.floor(Math.random() * 5),
        peak_resolution_height: [720, 1080, 1440, 2160][
          Math.floor(Math.random() * 4)
        ],
        playback_duration_ms: (
          30000 + Math.floor(Math.random() * 7200000)
        ).toString(),
      },
      cdn_provider: randomPick(["cloudfront", "akamai", "fastly"]),
      abr_algorithm: randomPick(["default", "experimental_v2"]),
      timestamp: nowTimestamp(),
    },
  };

  const start = Date.now();
  const res = client.invoke(`${SERVICE}/IngestQoEEvent`, req);
  const elapsed = Date.now() - start;

  qoeLatency.add(elapsed);
  totalCount.add(1);
  errorRate.add(res.status !== grpc.StatusOK);

  check(res, {
    "IngestQoEEvent: status OK": (r) => r.status === grpc.StatusOK,
    "IngestQoEEvent: accepted": (r) =>
      r.status === grpc.StatusOK && r.message && r.message.accepted,
  });

  client.close();
}

// ---------------------------------------------------------------------------
// Summary
// ---------------------------------------------------------------------------

export function handleSummary(data) {
  const get = (name, stat) => data.metrics[name]?.values?.[stat];

  const expP50 = get("m2_exposure_latency", "p(50)")?.toFixed(2) || "N/A";
  const expP95 = get("m2_exposure_latency", "p(95)")?.toFixed(2) || "N/A";
  const expP99 = get("m2_exposure_latency", "p(99)")?.toFixed(2) || "N/A";
  const expMax = get("m2_exposure_latency", "max")?.toFixed(2) || "N/A";

  const metP50 = get("m2_metric_latency", "p(50)")?.toFixed(2) || "N/A";
  const metP95 = get("m2_metric_latency", "p(95)")?.toFixed(2) || "N/A";
  const metP99 = get("m2_metric_latency", "p(99)")?.toFixed(2) || "N/A";

  const qoeP99 = get("m2_qoe_latency", "p(99)")?.toFixed(2) || "N/A";
  const rewP99 = get("m2_reward_latency", "p(99)")?.toFixed(2) || "N/A";

  const errRate = get("m2_error_rate", "rate") || 0;
  const total = get("m2_total", "count") || 0;

  const grpcP99 = get("grpc_req_duration", "p(99)")?.toFixed(2) || "N/A";
  const grpcTotal = get("grpc_req_duration", "count") || 0;

  const totalRps = grpcTotal / (parseFloat(DURATION) || 60);

  // Check SLA pass/fail
  const expP99Val = parseFloat(expP99);
  const metP99Val = parseFloat(metP99);
  const expPass = !isNaN(expP99Val) && expP99Val < 10.0;
  const metPass = !isNaN(metP99Val) && metP99Val < 10.0;
  const errPass = errRate < 0.001;

  const report = `
=============================================================
  M2 EVENT PIPELINE — LOAD TEST REPORT
=============================================================
  Target:          ${TARGET_RPS} rps sustained for ${DURATION}
  Server:          ${ADDR}

  --- IngestExposure (40%) ---
  p50 latency:     ${expP50} ms
  p95 latency:     ${expP95} ms
  p99 latency:     ${expP99} ms   (SLA: < 10ms)  ${expPass ? "PASS" : "FAIL"}
  max latency:     ${expMax} ms

  --- IngestMetricEvent (30%) ---
  p50 latency:     ${metP50} ms
  p95 latency:     ${metP95} ms
  p99 latency:     ${metP99} ms   (SLA: < 10ms)  ${metPass ? "PASS" : "FAIL"}

  --- IngestRewardEvent (15%) ---
  p99 latency:     ${rewP99} ms

  --- IngestQoEEvent (15%) ---
  p99 latency:     ${qoeP99} ms

  --- Overall ---
  Total requests:  ${grpcTotal}
  Achieved rps:    ${totalRps.toFixed(0)}
  gRPC p99:        ${grpcP99} ms
  Error rate:      ${(errRate * 100).toFixed(3)}%  (SLA: < 0.1%) ${errPass ? "PASS" : "FAIL"}
=============================================================
  RESULT: ${expPass && metPass && errPass ? "PASS — All SLAs met" : "FAIL — SLA violation detected"}
=============================================================
`;
  console.log(report);

  // Write JSON results for programmatic validation
  return {
    stdout: report,
    "loadtest_pipeline_results.json": JSON.stringify(
      {
        exposure_p99_ms: parseFloat(expP99) || null,
        exposure_p50_ms: parseFloat(expP50) || null,
        metric_p99_ms: parseFloat(metP99) || null,
        metric_p50_ms: parseFloat(metP50) || null,
        qoe_p99_ms: parseFloat(qoeP99) || null,
        reward_p99_ms: parseFloat(rewP99) || null,
        total_requests: grpcTotal,
        total_rps: totalRps,
        error_rate: errRate,
        sla_exposure_pass: expPass,
        sla_metric_pass: metPass,
        sla_error_pass: errPass,
        all_pass: expPass && metPass && errPass,
      },
      null,
      2,
    ),
  };
}
