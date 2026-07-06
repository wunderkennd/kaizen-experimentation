// =============================================================================
// k6 Throughput Generator — M2 Event Pipeline: 100K events/sec sustained
// =============================================================================
// Issue #502 (Phase 4 Validation): drives TARGET_EPS events/sec into M2's
// gRPC ingest endpoint for DURATION. Unlike loadtest_pipeline.js (latency
// SLA, unary calls), this script maximizes event throughput via the batch
// RPCs and emits an accounting summary consumed by the pass/fail gate
// (scripts/m2_throughput_watch.py evaluate) alongside Redpanda offset data.
//
// Event mix mirrors loadtest_pipeline.js: exposure 40%, metric 30%,
// reward 15%, qoe 15%. Exposure/metric/qoe use their batch RPCs; reward has
// no batch RPC in pipeline_service.proto, so it is driven unary.
//
// Every event_id is unique (RUN_ID + VU + iteration + index) so the Bloom
// dedup stage never drops synthetic events — a prerequisite for the
// zero-message-loss check downstream.
//
// Usage (normally driven by scripts/loadtest_m2_throughput.sh):
//   k6 run --env TARGET_EPS=100000 --env DURATION=330s --env RUN_ID=$(date +%s) \
//     scripts/loadtest_m2_throughput.js
// =============================================================================

import grpc from "k6/net/grpc";
import { check } from "k6";
import { Rate, Trend, Counter } from "k6/metrics";

const eventsSent = new Counter("m2t_events_sent");
const eventsAccepted = new Counter("m2t_events_accepted");
const eventsDuplicate = new Counter("m2t_events_duplicate");
const eventsInvalid = new Counter("m2t_events_invalid");
const errorRate = new Rate("m2t_error_rate");
const batchLatency = new Trend("m2t_batch_latency", true);

const ADDR = __ENV.PIPELINE_ADDR || "localhost:50052";
const TARGET_EPS = parseInt(__ENV.TARGET_EPS || "100000");
const DURATION = __ENV.DURATION || "330s";
const BATCH_SIZE = parseInt(__ENV.BATCH_SIZE || "100");
const RUN_ID = __ENV.RUN_ID || "m2t";
const PLAINTEXT = (__ENV.PLAINTEXT || "true") === "true";
const SUMMARY_PATH = __ENV.K6_SUMMARY_PATH || "m2_throughput_k6.json";

const SERVICE = "experimentation.pipeline.v1.EventIngestionService";

const client = new grpc.Client();
client.load(["proto"], "experimentation/pipeline/v1/pipeline_service.proto");

// Requests/sec per scenario: batch scenarios divide their event share by
// BATCH_SIZE; reward stays unary. ceil() over-provisions slightly, which is
// fine — TARGET_EPS is a floor, and the gate measures actual offset advance.
const rps = {
  exposure: Math.ceil((TARGET_EPS * 0.4) / BATCH_SIZE),
  metric: Math.ceil((TARGET_EPS * 0.3) / BATCH_SIZE),
  qoe: Math.ceil((TARGET_EPS * 0.15) / BATCH_SIZE),
  reward: Math.ceil(TARGET_EPS * 0.15),
};

// VU pools sized from request rate × latency budget (batch ≈ 50ms, unary ≈ 20ms).
function vus(rate, budgetSec, floor) {
  return Math.max(floor, Math.ceil(rate * budgetSec));
}

function scenario(exec, rate, budgetSec) {
  return {
    executor: "constant-arrival-rate",
    rate: rate,
    timeUnit: "1s",
    duration: DURATION,
    preAllocatedVUs: vus(rate, budgetSec, 10),
    maxVUs: vus(rate, budgetSec * 3, 30),
    exec: exec,
    gracefulStop: "10s",
  };
}

export const options = {
  scenarios: {
    exposure_batch: scenario("ingestExposureBatch", rps.exposure, 0.05),
    metric_batch: scenario("ingestMetricEventBatch", rps.metric, 0.05),
    qoe_batch: scenario("ingestQoEEventBatch", rps.qoe, 0.05),
    reward_unary: scenario("ingestRewardEvent", rps.reward, 0.02),
  },
  thresholds: {
    m2t_error_rate: ["rate < 0.001"],
  },
};

const EXPERIMENTS = ["exp_dev_001", "exp_dev_002", "exp_dev_003", "exp_dev_005", "exp_dev_006"];
const VARIANTS = ["control", "treatment_a", "treatment_b"];
const PLATFORMS = ["web", "ios", "android", "tv"];
const EVENT_TYPES = ["play_start", "watch_complete", "search", "add_to_list", "browse"];

function pick(arr) {
  return arr[Math.floor(Math.random() * arr.length)];
}

function ts() {
  return { seconds: Math.floor(Date.now() / 1000).toString(), nanos: 0 };
}

function eid(kind, i) {
  return `${RUN_ID}-${kind}-${__VU}-${__ITER}-${i}`;
}

let connected = false;
function conn() {
  if (!connected) {
    client.connect(ADDR, { plaintext: PLAINTEXT, timeout: "5s" });
    connected = true;
  }
}

// Invoke a batch RPC and record IngestBatchResponse accounting.
function invokeBatch(method, req, size) {
  const start = Date.now();
  const res = client.invoke(`${SERVICE}/${method}`, req);
  batchLatency.add(Date.now() - start);

  eventsSent.add(size);
  const ok = res.status === grpc.StatusOK;
  errorRate.add(!ok);
  if (ok && res.message) {
    eventsAccepted.add(parseInt(res.message.accepted_count || 0));
    eventsDuplicate.add(parseInt(res.message.duplicate_count || 0));
    eventsInvalid.add(parseInt(res.message.invalid_count || 0));
  }
  check(res, { [`${method}: status OK`]: () => ok });
}

export function ingestExposureBatch() {
  conn();
  const events = [];
  for (let i = 0; i < BATCH_SIZE; i++) {
    events.push({
      event_id: eid("exp", i),
      experiment_id: pick(EXPERIMENTS),
      user_id: `user-${Math.floor(Math.random() * 1_000_000)}`,
      variant_id: pick(VARIANTS),
      timestamp: ts(),
      platform: pick(PLATFORMS),
      session_id: `session-${Math.floor(Math.random() * 100_000)}`,
    });
  }
  invokeBatch("IngestExposureBatch", { events: events }, BATCH_SIZE);
}

export function ingestMetricEventBatch() {
  conn();
  const events = [];
  for (let i = 0; i < BATCH_SIZE; i++) {
    events.push({
      event_id: eid("met", i),
      user_id: `user-${Math.floor(Math.random() * 1_000_000)}`,
      event_type: pick(EVENT_TYPES),
      value: Math.random() * 3600,
      content_id: `content-${Math.floor(Math.random() * 1000)}`,
      session_id: `session-${Math.floor(Math.random() * 100_000)}`,
      timestamp: ts(),
    });
  }
  invokeBatch("IngestMetricEventBatch", { events: events }, BATCH_SIZE);
}

export function ingestQoEEventBatch() {
  conn();
  const events = [];
  for (let i = 0; i < BATCH_SIZE; i++) {
    events.push({
      event_id: eid("qoe", i),
      session_id: `session-${Math.floor(Math.random() * 100_000)}`,
      content_id: `content-${Math.floor(Math.random() * 1000)}`,
      user_id: `user-${Math.floor(Math.random() * 1_000_000)}`,
      metrics: {
        time_to_first_frame_ms: Math.floor(Math.random() * 5000).toString(),
        rebuffer_count: Math.floor(Math.random() * 10),
        rebuffer_ratio: Math.random() * 0.1,
        avg_bitrate_kbps: 2000 + Math.floor(Math.random() * 8000),
        resolution_switches: Math.floor(Math.random() * 5),
        peak_resolution_height: [720, 1080, 1440, 2160][Math.floor(Math.random() * 4)],
        playback_duration_ms: (30000 + Math.floor(Math.random() * 7200000)).toString(),
      },
      cdn_provider: pick(["cloudfront", "akamai", "fastly"]),
      abr_algorithm: pick(["default", "experimental_v2"]),
      timestamp: ts(),
    });
  }
  invokeBatch("IngestQoEEventBatch", { events: events }, BATCH_SIZE);
}

export function ingestRewardEvent() {
  conn();
  const res = client.invoke(`${SERVICE}/IngestRewardEvent`, {
    event: {
      event_id: eid("rew", 0),
      experiment_id: pick(EXPERIMENTS),
      user_id: `user-${Math.floor(Math.random() * 1_000_000)}`,
      arm_id: `arm-${Math.floor(Math.random() * 4)}`,
      reward: Math.random(),
      timestamp: ts(),
    },
  });
  eventsSent.add(1);
  const ok = res.status === grpc.StatusOK;
  errorRate.add(!ok);
  if (ok && res.message && res.message.accepted) {
    eventsAccepted.add(1);
  }
  check(res, { "IngestRewardEvent: status OK": () => ok });
}

export function handleSummary(data) {
  const count = (name) => data.metrics[name]?.values?.count || 0;
  const durationSec = (data.state?.testRunDurationMs || 0) / 1000;
  const accepted = count("m2t_events_accepted");

  const summary = {
    run_id: RUN_ID,
    target_eps: TARGET_EPS,
    batch_size: BATCH_SIZE,
    duration_sec: durationSec,
    events_sent: count("m2t_events_sent"),
    events_accepted: accepted,
    events_duplicate: count("m2t_events_duplicate"),
    events_invalid: count("m2t_events_invalid"),
    dropped_iterations: count("dropped_iterations"),
    error_rate: data.metrics["m2t_error_rate"]?.values?.rate || 0,
    accepted_eps: durationSec > 0 ? accepted / durationSec : 0,
    batch_p99_ms: data.metrics["m2t_batch_latency"]?.values?.["p(99)"] || null,
  };

  const out = {};
  out[SUMMARY_PATH] = JSON.stringify(summary, null, 2);
  out.stdout =
    `\n[m2-throughput] sent=${summary.events_sent} accepted=${summary.events_accepted}` +
    ` dup=${summary.events_duplicate} invalid=${summary.events_invalid}` +
    ` dropped_iters=${summary.dropped_iterations}` +
    ` accepted_eps=${summary.accepted_eps.toFixed(0)} (target ${TARGET_EPS})\n`;
  return out;
}
