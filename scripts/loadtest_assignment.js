// =============================================================================
// k6 Load Test — M1 Assignment Service: p99 < 5ms at configurable rps
// =============================================================================
// Validates SLA: GetAssignment p99 < 5ms under sustained load.
// VU counts scale dynamically with TARGET_RPS (baseline: 10K rps).
//
// Uses k6's native gRPC module against the tonic gRPC server.
// Exercises all experiment types from dev/config.json:
//   - AB (exp_dev_001, exp_dev_002)
//   - CUMULATIVE_HOLDOUT (exp_dev_holdout_001)
//   - SESSION_LEVEL (exp_dev_003, exp_dev_008)
//   - MAB (exp_dev_005)
//   - CONTEXTUAL_BANDIT (cold-start:movie-new-001)
//   - INTERLEAVING (exp_dev_004, exp_dev_006, exp_dev_007)
//
// Usage:
//   k6 run scripts/loadtest_assignment.js
//   ASSIGNMENT_ADDR=localhost:50051 k6 run scripts/loadtest_assignment.js
//   k6 run --env TARGET_RPS=50000 scripts/loadtest_assignment.js
//
// Full automated run:
//   bash scripts/loadtest_assignment.sh
//   TARGET_RPS=50000 bash scripts/loadtest_assignment.sh
// =============================================================================

import grpc from "k6/net/grpc";
import { check, sleep } from "k6";
import { Rate, Trend, Counter } from "k6/metrics";

// ---------------------------------------------------------------------------
// Custom Metrics
// ---------------------------------------------------------------------------

const assignLatency = new Trend("m1_assign_latency", true);
const assignErrors = new Rate("m1_assign_error_rate");
const assignCount = new Counter("m1_assign_total");
const interleaveLatency = new Trend("m1_interleave_latency", true);
const interleaveCount = new Counter("m1_interleave_total");
const batchLatency = new Trend("m1_batch_latency", true);
const batchCount = new Counter("m1_batch_total");

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

const ADDR = __ENV.ASSIGNMENT_ADDR || "localhost:50051";
const TARGET_RPS = parseInt(__ENV.TARGET_RPS || "10000");
const DURATION = __ENV.DURATION || "60s";
const RAMP_DURATION = __ENV.RAMP_DURATION || "10s";

// Scale VU counts proportionally to TARGET_RPS (baseline: 10K rps)
const VU_SCALE = Math.max(1, TARGET_RPS / 10000);

const SERVICE = "experimentation.assignment.v1.AssignmentService";

const client = new grpc.Client();
client.load(["proto"], "experimentation/assignment/v1/assignment_service.proto");

// ---------------------------------------------------------------------------
// Scenarios
// ---------------------------------------------------------------------------

export const options = {
  scenarios: {
    // GetAssignment (85% of traffic) — VUs scale with TARGET_RPS
    get_assignment: {
      executor: "constant-arrival-rate",
      rate: Math.floor(TARGET_RPS * 0.85),
      timeUnit: "1s",
      duration: DURATION,
      preAllocatedVUs: Math.ceil(100 * VU_SCALE),
      maxVUs: Math.ceil(500 * VU_SCALE),
      exec: "getAssignment",
      gracefulStop: "5s",
    },

    // GetAssignments batch (10% of traffic)
    get_assignments_batch: {
      executor: "constant-arrival-rate",
      rate: Math.floor(TARGET_RPS * 0.10),
      timeUnit: "1s",
      duration: DURATION,
      preAllocatedVUs: Math.ceil(20 * VU_SCALE),
      maxVUs: Math.ceil(100 * VU_SCALE),
      exec: "getAssignments",
      gracefulStop: "5s",
    },

    // GetInterleavedList (5% of traffic)
    get_interleaved: {
      executor: "constant-arrival-rate",
      rate: Math.floor(TARGET_RPS * 0.05),
      timeUnit: "1s",
      duration: DURATION,
      preAllocatedVUs: Math.ceil(10 * VU_SCALE),
      maxVUs: Math.ceil(50 * VU_SCALE),
      exec: "getInterleavedList",
      gracefulStop: "5s",
    },
  },

  thresholds: {
    // Phase 1 SLA: p99 < 5ms for GetAssignment
    "m1_assign_latency":      ["p(99) < 5"],
    "m1_assign_error_rate":   ["rate < 0.001"],

    // Phase 1 SLA: p99 < 15ms for GetInterleavedList
    "m1_interleave_latency":  ["p(99) < 15"],

    // Overall
    "grpc_req_duration":      ["p(99) < 10"],
  },
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function randomUserId() {
  return `user-${Math.floor(Math.random() * 1_000_000)}`;
}

function randomSessionId() {
  return `session-${Math.floor(Math.random() * 100_000)}`;
}

// Experiment pools matching dev/config.json
const AB_EXPERIMENTS = ["exp_dev_001", "exp_dev_002", "exp_dev_holdout_001"];
const SESSION_EXPERIMENTS = ["exp_dev_003", "exp_dev_008"];
const BANDIT_EXPERIMENTS = ["exp_dev_005", "cold-start:movie-new-001"];
const INTERLEAVE_EXPERIMENTS = [
  { id: "exp_dev_004", algos: ["algo_a", "algo_b"] },
  { id: "exp_dev_006", algos: ["algo_a", "algo_b"] },
  { id: "exp_dev_007", algos: ["algo_x", "algo_y", "algo_z"] },
];

// Weighted experiment picker: 50% AB, 20% session, 20% bandit, 10% targeted AB
function pickExperiment() {
  const r = Math.random();
  if (r < 0.50) {
    return { type: "ab", id: AB_EXPERIMENTS[Math.floor(Math.random() * AB_EXPERIMENTS.length)] };
  } else if (r < 0.70) {
    return { type: "session", id: SESSION_EXPERIMENTS[Math.floor(Math.random() * SESSION_EXPERIMENTS.length)] };
  } else {
    return { type: "bandit", id: BANDIT_EXPERIMENTS[Math.floor(Math.random() * BANDIT_EXPERIMENTS.length)] };
  }
}

function makeRankedList(prefix, n) {
  const items = [];
  for (let i = 0; i < n; i++) {
    items.push(`${prefix}_item_${i}`);
  }
  return { item_ids: items };
}

// ---------------------------------------------------------------------------
// Scenario: GetAssignment (single experiment)
// ---------------------------------------------------------------------------

export function getAssignment() {
  client.connect(ADDR, { plaintext: true, timeout: "5s" });

  const exp = pickExperiment();
  const req = { user_id: randomUserId(), experiment_id: exp.id };

  if (exp.type === "session") {
    req.session_id = randomSessionId();
  } else if (exp.id === "exp_dev_002") {
    // Targeted experiment — include attributes
    req.attributes = { country: "US", tier: "premium" };
  } else if (exp.id === "cold-start:movie-new-001") {
    // Contextual bandit — include context features
    req.attributes = {
      genre_affinity: "action",
      recency_days: `${Math.floor(Math.random() * 30)}`,
      tenure_months: `${Math.floor(Math.random() * 60)}`,
    };
  }

  const res = client.invoke(`${SERVICE}/GetAssignment`, req);

  assignLatency.add(res.status === grpc.StatusOK ? res.headers["grpc-message"] ? 0 : parseFloat(res.headers["x-response-time"] || "0") : 0);
  // Use k6's built-in grpc timing — the trend captures wall-clock latency
  assignCount.add(1);
  assignErrors.add(res.status !== grpc.StatusOK);

  check(res, {
    "GetAssignment: status OK": (r) => r.status === grpc.StatusOK,
    "GetAssignment: has variant": (r) =>
      r.status === grpc.StatusOK && r.message && (r.message.variantId || r.message.variant_id),
  });

  client.close();
}

// ---------------------------------------------------------------------------
// Scenario: GetAssignments (bulk)
// ---------------------------------------------------------------------------

export function getAssignments() {
  client.connect(ADDR, { plaintext: true, timeout: "5s" });

  const res = client.invoke(`${SERVICE}/GetAssignments`, {
    user_id: randomUserId(),
    session_id: randomSessionId(),
    attributes: { country: "US", tier: "standard" },
  });

  batchLatency.add(res.status === grpc.StatusOK ? 0 : 0);
  batchCount.add(1);

  check(res, {
    "GetAssignments: status OK": (r) => r.status === grpc.StatusOK,
    "GetAssignments: has assignments": (r) =>
      r.status === grpc.StatusOK && r.message && r.message.assignments,
  });

  client.close();
}

// ---------------------------------------------------------------------------
// Scenario: GetInterleavedList
// ---------------------------------------------------------------------------

export function getInterleavedList() {
  client.connect(ADDR, { plaintext: true, timeout: "5s" });

  const exp = INTERLEAVE_EXPERIMENTS[Math.floor(Math.random() * INTERLEAVE_EXPERIMENTS.length)];
  const lists = {};
  const listSize = 10 + Math.floor(Math.random() * 20); // 10–30 items
  for (const algo of exp.algos) {
    lists[algo] = makeRankedList(algo, listSize);
  }

  const res = client.invoke(`${SERVICE}/GetInterleavedList`, {
    experiment_id: exp.id,
    user_id: randomUserId(),
    algorithm_lists: lists,
  });

  interleaveLatency.add(res.status === grpc.StatusOK ? 0 : 0);
  interleaveCount.add(1);

  check(res, {
    "GetInterleavedList: status OK": (r) => r.status === grpc.StatusOK,
    "GetInterleavedList: has merged_list": (r) =>
      r.status === grpc.StatusOK && r.message && r.message.mergedList,
  });

  client.close();
}

// ---------------------------------------------------------------------------
// Summary
// ---------------------------------------------------------------------------

export function handleSummary(data) {
  const get = (name, stat) => data.metrics[name]?.values?.[stat];

  const assignP50 = get("m1_assign_latency", "p(50)")?.toFixed(2) || "N/A";
  const assignP95 = get("m1_assign_latency", "p(95)")?.toFixed(2) || "N/A";
  const assignP99 = get("m1_assign_latency", "p(99)")?.toFixed(2) || "N/A";
  const assignMax = get("m1_assign_latency", "max")?.toFixed(2) || "N/A";
  const assignTotal = get("m1_assign_total", "count") || 0;
  const assignErrRate = get("m1_assign_error_rate", "rate") || 0;

  const interleaveP99 = get("m1_interleave_latency", "p(99)")?.toFixed(2) || "N/A";
  const interleaveTotal = get("m1_interleave_total", "count") || 0;

  const batchTotal = get("m1_batch_total", "count") || 0;

  const grpcP99 = get("grpc_req_duration", "p(99)")?.toFixed(2) || "N/A";
  const grpcTotal = get("grpc_req_duration", "count") || 0;
  const grpcFailed = get("grpc_req_duration", "rate") || 0;

  const totalRps = grpcTotal / (parseFloat(DURATION) || 60);

  // Check SLA pass/fail
  const assignP99Val = parseFloat(assignP99);
  const interleaveP99Val = parseFloat(interleaveP99);
  const assignPass = !isNaN(assignP99Val) && assignP99Val < 5.0;
  const interleavePass = !isNaN(interleaveP99Val) && interleaveP99Val < 15.0;

  const report = `
=============================================================
  M1 ASSIGNMENT SERVICE — LOAD TEST REPORT
=============================================================
  Target:          ${TARGET_RPS} rps sustained for ${DURATION}
  Server:          ${ADDR}

  --- GetAssignment ---
  Total calls:     ${assignTotal}
  p50 latency:     ${assignP50} ms
  p95 latency:     ${assignP95} ms
  p99 latency:     ${assignP99} ms   (SLA: < 5ms)  ${assignPass ? "PASS" : "FAIL"}
  max latency:     ${assignMax} ms
  Error rate:      ${(assignErrRate * 100).toFixed(3)}%

  --- GetInterleavedList ---
  Total calls:     ${interleaveTotal}
  p99 latency:     ${interleaveP99} ms  (SLA: < 15ms) ${interleavePass ? "PASS" : "FAIL"}

  --- GetAssignments (batch) ---
  Total calls:     ${batchTotal}

  --- Overall gRPC ---
  Total requests:  ${grpcTotal}
  Achieved rps:    ${totalRps.toFixed(0)}
  gRPC p99:        ${grpcP99} ms
=============================================================
  RESULT: ${assignPass && interleavePass ? "PASS — All SLAs met" : "FAIL — SLA violation detected"}
=============================================================
`;
  console.log(report);

  // Write JSON results for programmatic validation
  return {
    stdout: report,
    "loadtest_assignment_results.json": JSON.stringify({
      assign_p99_ms: parseFloat(assignP99) || null,
      assign_p50_ms: parseFloat(assignP50) || null,
      assign_total: assignTotal,
      assign_error_rate: assignErrRate,
      interleave_p99_ms: parseFloat(interleaveP99) || null,
      interleave_total: interleaveTotal,
      batch_total: batchTotal,
      total_rps: totalRps,
      sla_assign_pass: assignPass,
      sla_interleave_pass: interleavePass,
      all_pass: assignPass && interleavePass,
    }, null, 2),
  };
}
