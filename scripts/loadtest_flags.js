// =============================================================================
// k6 Load Test — M7 Feature Flag Service: p99 < 10ms at 20K rps
// =============================================================================
// Validates Phase 4 SLA: EvaluateFlag p99 < 10ms under sustained 20K rps load.
//
// Uses ConnectRPC HTTP POST against the flags service (port 50057).
// Two scenarios:
//   - EvaluateFlag (single): 80% of traffic — p99 < 10ms
//   - EvaluateFlags (bulk):  20% of traffic — p99 < 50ms
//
// Usage:
//   k6 run scripts/loadtest_flags.js
//   FLAGS_URL=http://localhost:50057 k6 run scripts/loadtest_flags.js
//   k6 run --env TARGET_RPS=10000 scripts/loadtest_flags.js
//
// Full automated run:
//   bash scripts/loadtest_flags.sh
// =============================================================================

import http from "k6/http";
import { check, sleep } from "k6";
import { Rate, Trend, Counter } from "k6/metrics";

// ---------------------------------------------------------------------------
// Custom Metrics
// ---------------------------------------------------------------------------

const evalLatency = new Trend("m7_eval_latency", true);
const evalErrors = new Rate("m7_eval_error_rate");
const evalCount = new Counter("m7_eval_total");
const bulkLatency = new Trend("m7_bulk_latency", true);
const bulkErrors = new Rate("m7_bulk_error_rate");
const bulkCount = new Counter("m7_bulk_total");

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

const FLAGS_URL = __ENV.FLAGS_URL || "http://localhost:50057";
const TARGET_RPS = parseInt(__ENV.TARGET_RPS || "20000");
const DURATION = __ENV.DURATION || "60s";
const RAMP_DURATION = __ENV.RAMP_DURATION || "10s";

const SERVICE = "experimentation.flags.v1.FeatureFlagService";

const CONNECT_HEADERS = {
  "Content-Type": "application/json",
};

// Flag IDs are seeded by the shell driver script before k6 starts.
// We use flag keys that the seed script creates.
const FLAG_IDS = (__ENV.FLAG_IDS || "").split(",").filter((id) => id.length > 0);

// Fallback: if no flag IDs provided, use a placeholder (server must have flags).
function getRandomFlagId() {
  if (FLAG_IDS.length > 0) {
    return FLAG_IDS[Math.floor(Math.random() * FLAG_IDS.length)];
  }
  return "loadtest-flag-0";
}

// ---------------------------------------------------------------------------
// Scenarios
// ---------------------------------------------------------------------------

export const options = {
  scenarios: {
    // Sustained 80% of target rps for single EvaluateFlag
    evaluate_flag: {
      executor: "constant-arrival-rate",
      rate: Math.floor(TARGET_RPS * 0.80),
      timeUnit: "1s",
      duration: DURATION,
      preAllocatedVUs: 200,
      maxVUs: 800,
      exec: "evaluateFlag",
      gracefulStop: "5s",
    },

    // Sustained 20% of target rps for bulk EvaluateFlags
    evaluate_flags_bulk: {
      executor: "constant-arrival-rate",
      rate: Math.floor(TARGET_RPS * 0.20),
      timeUnit: "1s",
      duration: DURATION,
      preAllocatedVUs: 50,
      maxVUs: 200,
      exec: "evaluateFlagsBulk",
      gracefulStop: "5s",
    },
  },

  thresholds: {
    // Phase 4 SLA: p99 < 10ms for EvaluateFlag
    "m7_eval_latency": ["p(99) < 10"],
    "m7_eval_error_rate": ["rate < 0.001"],

    // Bulk: p99 < 50ms for EvaluateFlags
    "m7_bulk_latency": ["p(99) < 50"],
    "m7_bulk_error_rate": ["rate < 0.001"],

    // Overall HTTP
    "http_req_duration": ["p(99) < 20"],
  },
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function randomUserId() {
  return `user-${Math.floor(Math.random() * 1_000_000)}`;
}

function connectPost(method, body) {
  const url = `${FLAGS_URL}/${SERVICE}/${method}`;
  return http.post(url, JSON.stringify(body), { headers: CONNECT_HEADERS });
}

// ---------------------------------------------------------------------------
// Scenario: EvaluateFlag (single flag)
// ---------------------------------------------------------------------------

export function evaluateFlag() {
  const res = connectPost("EvaluateFlag", {
    flag_id: getRandomFlagId(),
    user_id: randomUserId(),
  });

  evalLatency.add(res.timings.duration);
  evalCount.add(1);
  evalErrors.add(res.status !== 200);

  check(res, {
    "EvaluateFlag: status 200": (r) => r.status === 200,
    "EvaluateFlag: < 20ms": (r) => r.timings.duration < 20,
  });
}

// ---------------------------------------------------------------------------
// Scenario: EvaluateFlags (bulk — all enabled flags for a user)
// ---------------------------------------------------------------------------

export function evaluateFlagsBulk() {
  const res = connectPost("EvaluateFlags", {
    user_id: randomUserId(),
  });

  bulkLatency.add(res.timings.duration);
  bulkCount.add(1);
  bulkErrors.add(res.status !== 200);

  check(res, {
    "EvaluateFlags: status 200": (r) => r.status === 200,
    "EvaluateFlags: < 50ms": (r) => r.timings.duration < 50,
  });
}

// ---------------------------------------------------------------------------
// Summary
// ---------------------------------------------------------------------------

export function handleSummary(data) {
  const get = (name, stat) => data.metrics[name]?.values?.[stat];

  const evalP50 = get("m7_eval_latency", "p(50)")?.toFixed(2) || "N/A";
  const evalP95 = get("m7_eval_latency", "p(95)")?.toFixed(2) || "N/A";
  const evalP99 = get("m7_eval_latency", "p(99)")?.toFixed(2) || "N/A";
  const evalMax = get("m7_eval_latency", "max")?.toFixed(2) || "N/A";
  const evalTotal = get("m7_eval_total", "count") || 0;
  const evalErrRate = get("m7_eval_error_rate", "rate") || 0;

  const bulkP99 = get("m7_bulk_latency", "p(99)")?.toFixed(2) || "N/A";
  const bulkTotal = get("m7_bulk_total", "count") || 0;
  const bulkErrRate = get("m7_bulk_error_rate", "rate") || 0;

  const httpP99 = get("http_req_duration", "p(99)")?.toFixed(2) || "N/A";
  const httpTotal = get("http_req_duration", "count") || 0;

  const totalRps = httpTotal / (parseFloat(DURATION) || 60);

  // Check SLA pass/fail
  const evalP99Val = parseFloat(evalP99);
  const bulkP99Val = parseFloat(bulkP99);
  const evalPass = !isNaN(evalP99Val) && evalP99Val < 10.0;
  const bulkPass = !isNaN(bulkP99Val) && bulkP99Val < 50.0;

  const report = `
=============================================================
  M7 FLAG SERVICE — LOAD TEST REPORT
=============================================================
  Target:          ${TARGET_RPS} rps sustained for ${DURATION}
  Server:          ${FLAGS_URL}

  --- EvaluateFlag (single) ---
  Total calls:     ${evalTotal}
  p50 latency:     ${evalP50} ms
  p95 latency:     ${evalP95} ms
  p99 latency:     ${evalP99} ms   (SLA: < 10ms)  ${evalPass ? "PASS" : "FAIL"}
  max latency:     ${evalMax} ms
  Error rate:      ${(evalErrRate * 100).toFixed(3)}%

  --- EvaluateFlags (bulk) ---
  Total calls:     ${bulkTotal}
  p99 latency:     ${bulkP99} ms   (SLA: < 50ms)  ${bulkPass ? "PASS" : "FAIL"}
  Error rate:      ${(bulkErrRate * 100).toFixed(3)}%

  --- Overall HTTP ---
  Total requests:  ${httpTotal}
  Achieved rps:    ${totalRps.toFixed(0)}
  HTTP p99:        ${httpP99} ms
=============================================================
  RESULT: ${evalPass && bulkPass ? "PASS — All SLAs met" : "FAIL — SLA violation detected"}
=============================================================
`;
  console.log(report);

  // Write JSON results for programmatic validation
  return {
    stdout: report,
    "loadtest_flags_results.json": JSON.stringify(
      {
        eval_p99_ms: parseFloat(evalP99) || null,
        eval_p50_ms: parseFloat(evalP50) || null,
        eval_total: evalTotal,
        eval_error_rate: evalErrRate,
        bulk_p99_ms: parseFloat(bulkP99) || null,
        bulk_total: bulkTotal,
        bulk_error_rate: bulkErrRate,
        total_rps: totalRps,
        sla_eval_pass: evalPass,
        sla_bulk_pass: bulkPass,
        all_pass: evalPass && bulkPass,
      },
      null,
      2
    ),
  };
}
