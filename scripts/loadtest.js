// ==============================================================================
// k6 Load Test — Experimentation Platform
// ==============================================================================
// Usage:
//   make loadtest
//   or: k6 run scripts/loadtest.js
//   or: k6 run --vus 50 --duration 5m scripts/loadtest.js
//
// Scenarios:
//   1. Assignment: High-frequency variant lookups (p99 < 50ms SLO)
//   2. Exposure:   Event ingestion bursts
//   3. Management: CRUD operations (lower frequency)
//   4. Flags:      Flag evaluation lookups (p99 < 10ms SLO)
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
    "assignment_latency":    ["p(99) < 50"],
    "assignment_error_rate": ["rate < 0.001"],
    // Flag SLO: p99 < 10ms
    "flag_eval_latency":     ["p(99) < 10"],
    // Overall HTTP errors
    "http_req_failed":       ["rate < 0.01"],
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
// Scenario: Exposure Events
// ---------------------------------------------------------------------------

export function exposureScenario() {
  group("Pipeline — TrackExposure", () => {
    const res = connectPost(
      BASE_URLS.pipeline,
      "experimentation.pipeline.v1.PipelineService",
      "TrackExposure",
      {
        user_id: randomUserId(),
        experiment_id: randomExperimentId(),
        variant_name: Math.random() > 0.5 ? "control" : "treatment",
        timestamp: new Date().toISOString(),
        context: {
          session_id: `session-${Math.floor(Math.random() * 100_000)}`,
          platform: "web",
        },
      }
    );

    exposuresSent.add(1);
    check(res, { "exposure: status 200": (r) => r.status === 200 });
  });
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
  console.log(`Assignment p50:  ${get("assignment_latency", "p(50)")?.toFixed(2)} ms`);
  console.log(`Assignment p99:  ${get("assignment_latency", "p(99)")?.toFixed(2)} ms  (SLO: <50ms)`);
  console.log(`Assignment errs: ${(get("assignment_error_rate", "rate") * 100)?.toFixed(3)}%`);
  console.log(`Flag eval p50:   ${get("flag_eval_latency", "p(50)")?.toFixed(2)} ms`);
  console.log(`Flag eval p99:   ${get("flag_eval_latency", "p(99)")?.toFixed(2)} ms  (SLO: <10ms)`);
  console.log(`Exposures sent:  ${get("exposures_sent", "count")}`);
  console.log(`Total HTTP reqs: ${get("http_reqs", "count")}`);
  console.log(`HTTP fail rate:  ${(get("http_req_failed", "rate") * 100)?.toFixed(3)}%`);
  console.log("===================================================\n");

  return { stdout: JSON.stringify(data.metrics, null, 2) };
}
