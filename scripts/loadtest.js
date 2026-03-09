// ==============================================================================
// k6 Load Test — Experimentation Platform
// ==============================================================================
// Usage:
//   make loadtest
//   or: k6 run scripts/loadtest.js
//   or: k6 run --vus 50 --duration 5m scripts/loadtest.js
//
// Scenarios:
//   1. Assignment:    High-frequency variant lookups (p99 < 50ms SLO)
//   2. Exposure:      Event ingestion via IngestExposure RPC
//   3. Metric Event:  Metric event ingestion via IngestMetricEvent RPC
//   4. QoE Event:     QoE playback event ingestion via IngestQoEEvent RPC
//   5. Reward Event:  Bandit reward ingestion via IngestRewardEvent RPC
//   6. Batch:         Batch exposure ingestion via IngestExposureBatch RPC
//   7. Management:    CRUD operations (lower frequency)
//   8. Flags:         Flag evaluation lookups (p99 < 10ms SLO)
// ==============================================================================

import http from "k6/http";
import { check, sleep, group } from "k6";
import { Rate, Trend, Counter } from "k6/metrics";

// ---------------------------------------------------------------------------
// Custom Metrics
// ---------------------------------------------------------------------------

const assignmentLatency = new Trend("assignment_latency", true);
const assignmentErrors = new Rate("assignment_error_rate");
const flagLatency = new Trend("flag_eval_latency", true);
const exposuresSent = new Counter("exposures_sent");

// Pipeline-specific metrics
const pipelineIngestLatency = new Trend("pipeline_ingest_latency", true);
const pipelineErrors = new Rate("pipeline_error_rate");
const metricEventsSent = new Counter("metric_events_sent");
const qoeEventsSent = new Counter("qoe_events_sent");
const rewardEventsSent = new Counter("reward_events_sent");
const batchEventsSent = new Counter("batch_events_sent");

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

const BASE_URLS = {
  assignment: __ENV.ASSIGNMENT_URL || "http://localhost:50051",
  pipeline:   __ENV.PIPELINE_URL   || "http://localhost:50052",
  management: __ENV.MANAGEMENT_URL || "http://localhost:50055",
  flags:      __ENV.FLAGS_URL      || "http://localhost:50057",
};

// ConnectRPC uses HTTP POST with JSON body + specific content-type
const CONNECT_HEADERS = {
  "Content-Type": "application/json",
};

const PIPELINE_SERVICE = "experimentation.pipeline.v1.EventIngestionService";

export const options = {
  scenarios: {
    // --- Assignment: sustained high-throughput ---------------------------------
    assignment_load: {
      executor: "ramping-vus",
      startVUs: 5,
      stages: [
        { duration: "30s", target: 20 },   // ramp up
        { duration: "2m",  target: 50 },   // sustained peak
        { duration: "30s", target: 100 },  // spike
        { duration: "1m",  target: 50 },   // return to normal
        { duration: "30s", target: 0 },    // ramp down
      ],
      exec: "assignmentScenario",
      gracefulStop: "10s",
    },

    // --- Exposure events: burst ingestion -------------------------------------
    exposure_burst: {
      executor: "constant-arrival-rate",
      rate: 500,
      timeUnit: "1s",
      duration: "3m",
      preAllocatedVUs: 30,
      maxVUs: 60,
      exec: "exposureScenario",
      startTime: "30s",
    },

    // --- Metric events: sustained ingestion -----------------------------------
    metric_event_load: {
      executor: "constant-arrival-rate",
      rate: 300,
      timeUnit: "1s",
      duration: "3m",
      preAllocatedVUs: 20,
      maxVUs: 40,
      exec: "metricEventScenario",
      startTime: "30s",
    },

    // --- QoE events: moderate ingestion ---------------------------------------
    qoe_event_load: {
      executor: "constant-arrival-rate",
      rate: 100,
      timeUnit: "1s",
      duration: "3m",
      preAllocatedVUs: 10,
      maxVUs: 20,
      exec: "qoeEventScenario",
      startTime: "30s",
    },

    // --- Reward events: low-frequency bandit rewards --------------------------
    reward_event_load: {
      executor: "constant-arrival-rate",
      rate: 50,
      timeUnit: "1s",
      duration: "3m",
      preAllocatedVUs: 5,
      maxVUs: 15,
      exec: "rewardEventScenario",
      startTime: "30s",
    },

    // --- Batch exposure: throughput comparison --------------------------------
    batch_exposure: {
      executor: "constant-vus",
      vus: 10,
      duration: "2m",
      exec: "batchScenario",
      startTime: "1m",
    },

    // --- Management CRUD: low-frequency background ----------------------------
    management_crud: {
      executor: "per-vu-iterations",
      vus: 3,
      iterations: 20,
      exec: "managementScenario",
      startTime: "10s",
    },

    // --- Flag evaluation: high-frequency, low-latency -------------------------
    flag_evaluation: {
      executor: "constant-vus",
      vus: 20,
      duration: "3m",
      exec: "flagScenario",
      startTime: "15s",
    },
  },

  thresholds: {
    // Assignment SLO: p99 < 50ms
    "assignment_latency":       ["p(99) < 50"],
    "assignment_error_rate":    ["rate < 0.001"],
    // Flag SLO: p99 < 10ms
    "flag_eval_latency":        ["p(99) < 10"],
    // Pipeline SLO: p99 < 10ms for single-event ingestion
    "pipeline_ingest_latency":  ["p(99) < 10"],
    "pipeline_error_rate":      ["rate < 0.01"],
    // Overall HTTP errors
    "http_req_failed":          ["rate < 0.01"],
  },
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function randomUserId() {
  return `user-${Math.floor(Math.random() * 1_000_000)}`;
}

function randomExperimentId() {
  const experiments = [
    "homepage_recs_v2",
    "search_ranking_v3",
    "player_ui_experiment",
    "content_cold_start_bandit",
    "checkout_flow_v2",
    "personalization_v4",
    "onboarding_experiment",
  ];
  return experiments[Math.floor(Math.random() * experiments.length)];
}

function randomContentId() {
  return `content_${String(Math.floor(Math.random() * 200) + 1).padStart(4, "0")}`;
}

function randomSessionId() {
  return `session-${Math.floor(Math.random() * 100_000)}`;
}

function nowTimestamp() {
  const now = Math.floor(Date.now() / 1000);
  return { seconds: `${now}`, nanos: Math.floor(Math.random() * 999_999_999) };
}

function connectPost(baseUrl, service, method, body) {
  const url = `${baseUrl}/${service}/${method}`;
  return http.post(url, JSON.stringify(body), { headers: CONNECT_HEADERS });
}

// ---------------------------------------------------------------------------
// Scenario: Assignment
// ---------------------------------------------------------------------------

export function assignmentScenario() {
  group("Assignment — GetAssignment", () => {
    const res = connectPost(
      BASE_URLS.assignment,
      "experimentation.assignment.v1.AssignmentService",
      "GetAssignment",
      {
        experiment_id: randomExperimentId(),
        user_id: randomUserId(),
        context: { platform: "web", country: "US", device_type: "desktop" },
      }
    );

    assignmentLatency.add(res.timings.duration);
    assignmentErrors.add(res.status !== 200);

    check(res, {
      "assignment: status 200": (r) => r.status === 200,
      "assignment: has body":   (r) => r.body && r.body.length > 0,
      "assignment: < 100ms":    (r) => r.timings.duration < 100,
    });
  });

  sleep(Math.random() * 0.1);
}

// ---------------------------------------------------------------------------
// Scenario: Exposure Events (IngestExposure)
// ---------------------------------------------------------------------------

export function exposureScenario() {
  group("Pipeline — IngestExposure", () => {
    const res = connectPost(
      BASE_URLS.pipeline,
      PIPELINE_SERVICE,
      "IngestExposure",
      {
        event: {
          event_id: `exp-${Date.now()}-${Math.floor(Math.random() * 1_000_000)}`,
          experiment_id: randomExperimentId(),
          user_id: randomUserId(),
          variant_id: Math.random() > 0.5 ? "control" : "treatment",
          timestamp: nowTimestamp(),
          platform: "web",
          session_id: randomSessionId(),
        },
      }
    );

    pipelineIngestLatency.add(res.timings.duration);
    pipelineErrors.add(res.status !== 200);
    exposuresSent.add(1);

    check(res, {
      "exposure: status 200": (r) => r.status === 200,
      "exposure: accepted":   (r) => {
        try { return JSON.parse(r.body).accepted === true; }
        catch { return false; }
      },
    });
  });
}

// ---------------------------------------------------------------------------
// Scenario: Metric Events (IngestMetricEvent)
// ---------------------------------------------------------------------------

export function metricEventScenario() {
  const metricTypes = [
    "watch_time_minutes",
    "sessions_per_day",
    "content_completion_rate",
    "search_result_clicks",
    "playback_start_rate",
    "engagement_score",
  ];

  group("Pipeline — IngestMetricEvent", () => {
    const metricType = metricTypes[Math.floor(Math.random() * metricTypes.length)];
    const res = connectPost(
      BASE_URLS.pipeline,
      PIPELINE_SERVICE,
      "IngestMetricEvent",
      {
        event: {
          event_id: `met-${Date.now()}-${Math.floor(Math.random() * 1_000_000)}`,
          user_id: randomUserId(),
          event_type: metricType,
          value: Math.random() * 100,
          content_id: randomContentId(),
          session_id: randomSessionId(),
          timestamp: nowTimestamp(),
          properties: { source: "k6_loadtest" },
        },
      }
    );

    pipelineIngestLatency.add(res.timings.duration);
    pipelineErrors.add(res.status !== 200);
    metricEventsSent.add(1);

    check(res, {
      "metric_event: status 200": (r) => r.status === 200,
      "metric_event: accepted":   (r) => {
        try { return JSON.parse(r.body).accepted === true; }
        catch { return false; }
      },
    });
  });
}

// ---------------------------------------------------------------------------
// Scenario: QoE Events (IngestQoEEvent)
// ---------------------------------------------------------------------------

export function qoeEventScenario() {
  const cdnProviders = ["cloudfront", "akamai", "fastly", "cloudflare"];
  const abrAlgorithms = ["buffer_based", "rate_based", "hybrid_abr", "low_latency"];
  const encodingProfiles = ["h264_baseline", "h264_high", "h265_main", "av1_main"];

  group("Pipeline — IngestQoEEvent", () => {
    const avgBitrate = 2000 + Math.floor(Math.random() * 12000);
    const rebufferCount = Math.floor(Math.random() * 5);
    const playbackDurationMs = 30000 + Math.floor(Math.random() * 7200000);
    const rebufferRatio = rebufferCount > 0
      ? Math.min(1.0, (rebufferCount * (500 + Math.random() * 2500)) / playbackDurationMs)
      : 0.0;

    const res = connectPost(
      BASE_URLS.pipeline,
      PIPELINE_SERVICE,
      "IngestQoEEvent",
      {
        event: {
          event_id: `qoe-${Date.now()}-${Math.floor(Math.random() * 1_000_000)}`,
          session_id: randomSessionId(),
          content_id: randomContentId(),
          user_id: randomUserId(),
          metrics: {
            time_to_first_frame_ms: `${500 + Math.floor(Math.random() * 4000)}`,
            rebuffer_count: rebufferCount,
            rebuffer_ratio: rebufferRatio,
            avg_bitrate_kbps: avgBitrate,
            resolution_switches: Math.floor(Math.random() * 10),
            peak_resolution_height: avgBitrate > 8000 ? 1080 : 720,
            startup_failure_rate: Math.random() < 0.02 ? 1.0 : 0.0,
            playback_duration_ms: `${playbackDurationMs}`,
          },
          cdn_provider: cdnProviders[Math.floor(Math.random() * cdnProviders.length)],
          abr_algorithm: abrAlgorithms[Math.floor(Math.random() * abrAlgorithms.length)],
          encoding_profile: encodingProfiles[Math.floor(Math.random() * encodingProfiles.length)],
          timestamp: nowTimestamp(),
        },
      }
    );

    pipelineIngestLatency.add(res.timings.duration);
    pipelineErrors.add(res.status !== 200);
    qoeEventsSent.add(1);

    check(res, {
      "qoe_event: status 200": (r) => r.status === 200,
      "qoe_event: accepted":   (r) => {
        try { return JSON.parse(r.body).accepted === true; }
        catch { return false; }
      },
    });
  });
}

// ---------------------------------------------------------------------------
// Scenario: Reward Events (IngestRewardEvent)
// ---------------------------------------------------------------------------

export function rewardEventScenario() {
  const banditArms = ["arm_0", "arm_1", "arm_2", "arm_3"];

  group("Pipeline — IngestRewardEvent", () => {
    // Binary reward (70%) or continuous [0,1] (30%)
    const reward = Math.random() < 0.7
      ? (Math.random() < 0.3 ? 1.0 : 0.0)
      : Math.random();

    const res = connectPost(
      BASE_URLS.pipeline,
      PIPELINE_SERVICE,
      "IngestRewardEvent",
      {
        event: {
          event_id: `rwd-${Date.now()}-${Math.floor(Math.random() * 1_000_000)}`,
          experiment_id: "content_cold_start_bandit",
          user_id: randomUserId(),
          arm_id: banditArms[Math.floor(Math.random() * banditArms.length)],
          reward: reward,
          timestamp: nowTimestamp(),
          context_json: JSON.stringify({ genre: "drama", hour: new Date().getHours() }),
        },
      }
    );

    pipelineIngestLatency.add(res.timings.duration);
    pipelineErrors.add(res.status !== 200);
    rewardEventsSent.add(1);

    check(res, {
      "reward_event: status 200": (r) => r.status === 200,
      "reward_event: accepted":   (r) => {
        try { return JSON.parse(r.body).accepted === true; }
        catch { return false; }
      },
    });
  });
}

// ---------------------------------------------------------------------------
// Scenario: Batch Exposure Ingestion (IngestExposureBatch)
// ---------------------------------------------------------------------------

export function batchScenario() {
  const batchSize = 50;

  group("Pipeline — IngestExposureBatch", () => {
    const events = [];
    for (let i = 0; i < batchSize; i++) {
      events.push({
        event_id: `batch-${Date.now()}-${__VU}-${__ITER}-${i}`,
        experiment_id: randomExperimentId(),
        user_id: randomUserId(),
        variant_id: Math.random() > 0.5 ? "control" : "treatment",
        timestamp: nowTimestamp(),
        platform: "web",
        session_id: randomSessionId(),
      });
    }

    const res = connectPost(
      BASE_URLS.pipeline,
      PIPELINE_SERVICE,
      "IngestExposureBatch",
      { events: events }
    );

    pipelineIngestLatency.add(res.timings.duration);
    pipelineErrors.add(res.status !== 200);
    batchEventsSent.add(batchSize);

    check(res, {
      "batch: status 200": (r) => r.status === 200,
      "batch: has accepted_count": (r) => {
        try { return JSON.parse(r.body).accepted_count > 0; }
        catch { return false; }
      },
    });
  });

  sleep(0.5);
}

// ---------------------------------------------------------------------------
// Scenario: Management CRUD
// ---------------------------------------------------------------------------

export function managementScenario() {
  group("Management — CreateExperiment", () => {
    const res = connectPost(
      BASE_URLS.management,
      "experimentation.management.v1.ManagementService",
      "CreateExperiment",
      {
        name: `Load Test Experiment ${__VU}-${__ITER}`,
        description: "Created by k6 load test",
        experiment_type: "EXPERIMENT_TYPE_AB",
        owner_email: "loadtest@example.com",
        variants: [
          { name: "control",   is_control: true,  traffic_fraction: 0.5 },
          { name: "treatment", is_control: false, traffic_fraction: 0.5 },
        ],
        primary_metric_id: "metric_conversion_rate",
      }
    );
    check(res, { "create: status 200": (r) => r.status === 200 });
  });

  sleep(0.5);

  group("Management — ListExperiments", () => {
    const res = connectPost(
      BASE_URLS.management,
      "experimentation.management.v1.ManagementService",
      "ListExperiments",
      { page_size: 10 }
    );
    check(res, { "list: status 200": (r) => r.status === 200 });
  });

  sleep(1);
}

// ---------------------------------------------------------------------------
// Scenario: Flag Evaluation
// ---------------------------------------------------------------------------

export function flagScenario() {
  group("Flags — EvaluateFlag", () => {
    const res = connectPost(
      BASE_URLS.flags,
      "experimentation.flags.v1.FlagService",
      "EvaluateFlag",
      {
        flag_key: "feature_new_player_ui",
        user_id: randomUserId(),
        context: { platform: "ios", app_version: "4.2.1", country: "US" },
      }
    );

    flagLatency.add(res.timings.duration);
    check(res, {
      "flag: status 200": (r) => r.status === 200,
      "flag: < 20ms":     (r) => r.timings.duration < 20,
    });
  });

  sleep(Math.random() * 0.05);
}

// ---------------------------------------------------------------------------
// Summary
// ---------------------------------------------------------------------------

export function handleSummary(data) {
  const get = (name, stat) => data.metrics[name]?.values?.[stat];

  console.log("\n=== Experimentation Platform Load Test Summary ===");
  console.log("");
  console.log("--- Assignment ---");
  console.log(`  p50:  ${get("assignment_latency", "p(50)")?.toFixed(2)} ms`);
  console.log(`  p99:  ${get("assignment_latency", "p(99)")?.toFixed(2)} ms  (SLO: <50ms)`);
  console.log(`  errs: ${(get("assignment_error_rate", "rate") * 100)?.toFixed(3)}%`);
  console.log("");
  console.log("--- Pipeline (all event types) ---");
  console.log(`  p50:  ${get("pipeline_ingest_latency", "p(50)")?.toFixed(2)} ms`);
  console.log(`  p99:  ${get("pipeline_ingest_latency", "p(99)")?.toFixed(2)} ms  (SLO: <10ms)`);
  console.log(`  errs: ${(get("pipeline_error_rate", "rate") * 100)?.toFixed(3)}%`);
  console.log("");
  console.log("--- Pipeline Event Counts ---");
  console.log(`  Exposures (single): ${get("exposures_sent", "count")}`);
  console.log(`  Metric events:      ${get("metric_events_sent", "count")}`);
  console.log(`  QoE events:         ${get("qoe_events_sent", "count")}`);
  console.log(`  Reward events:      ${get("reward_events_sent", "count")}`);
  console.log(`  Batch events:       ${get("batch_events_sent", "count")}`);
  console.log("");
  console.log("--- Flag Evaluation ---");
  console.log(`  p50:  ${get("flag_eval_latency", "p(50)")?.toFixed(2)} ms`);
  console.log(`  p99:  ${get("flag_eval_latency", "p(99)")?.toFixed(2)} ms  (SLO: <10ms)`);
  console.log("");
  console.log("--- Overall ---");
  console.log(`  Total HTTP reqs: ${get("http_reqs", "count")}`);
  console.log(`  HTTP fail rate:  ${(get("http_req_failed", "rate") * 100)?.toFixed(3)}%`);
  console.log("===================================================\n");

  return { stdout: JSON.stringify(data.metrics, null, 2) };
}
