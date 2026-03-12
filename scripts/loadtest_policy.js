// =============================================================================
// k6 Load Test — M4b Bandit Policy Service: p99 < 15ms at 10K rps
// =============================================================================
// Phase 4 SLA validation. Validates SelectArm p99 < 15ms under sustained 10K rps.
//
// Uses k6's native gRPC module against the tonic gRPC server.
// All experiments are LinUCB (created via CreateColdStartBandit), so every
// SelectArm call includes context_features (user_age_bucket, watch_history_len,
// subscription_tier). This exercises the O(d^2) matrix path per arm.
//
// Usage:
//   k6 run scripts/loadtest_policy.js
//   POLICY_ADDR=localhost:50054 k6 run scripts/loadtest_policy.js
//   k6 run --env TARGET_RPS=5000 scripts/loadtest_policy.js
//
// Full automated run:
//   bash scripts/loadtest_policy.sh
// =============================================================================

import grpc from "k6/net/grpc";
import { check } from "k6";
import { Rate, Counter } from "k6/metrics";

// ---------------------------------------------------------------------------
// Custom Metrics
// ---------------------------------------------------------------------------

const selectArmErrors = new Rate("m4b_select_arm_error_rate");
const selectArmCount = new Counter("m4b_select_arm_total");
const mgmtErrors = new Rate("m4b_mgmt_error_rate");
const mgmtCount = new Counter("m4b_mgmt_total");

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

const ADDR = __ENV.POLICY_ADDR || "localhost:50054";
const TARGET_RPS = parseInt(__ENV.TARGET_RPS || "10000");
const DURATION = __ENV.DURATION || "60s";

const SERVICE = "experimentation.bandit.v1.BanditPolicyService";

// Experiment IDs passed from orchestration script (comma-separated)
const EXPERIMENT_IDS = (__ENV.EXPERIMENT_IDS || "cold-start:loadtest-0").split(",");

const client = new grpc.Client();
client.load(["proto"], "experimentation/bandit/v1/bandit_service.proto");

// ---------------------------------------------------------------------------
// Scenarios
// ---------------------------------------------------------------------------

export const options = {
  scenarios: {
    // Sustained SelectArm: 95% of target RPS
    select_arm: {
      executor: "constant-arrival-rate",
      rate: Math.floor(TARGET_RPS * 0.95),
      timeUnit: "1s",
      duration: DURATION,
      preAllocatedVUs: 100,
      maxVUs: 500,
      exec: "selectArm",
      gracefulStop: "5s",
    },

    // Management RPCs: 5% of target RPS (ExportAffinityScores + GetPolicySnapshot)
    management: {
      executor: "constant-arrival-rate",
      rate: Math.floor(TARGET_RPS * 0.05),
      timeUnit: "1s",
      duration: DURATION,
      preAllocatedVUs: 10,
      maxVUs: 50,
      exec: "managementRpc",
      gracefulStop: "5s",
    },
  },

  thresholds: {
    // Phase 4 SLA: p99 < 15ms for SelectArm
    "grpc_req_duration": ["p(99) < 15"],
    // Error rate < 0.1%
    "m4b_select_arm_error_rate": ["rate < 0.001"],
  },
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function randomUserId() {
  return `user-${Math.floor(Math.random() * 1_000_000)}`;
}

function randomExperimentId() {
  return EXPERIMENT_IDS[Math.floor(Math.random() * EXPERIMENT_IDS.length)];
}

// Generate context features matching DEFAULT_COLD_START_FEATURES in grpc.rs
function randomContextFeatures() {
  return {
    user_age_bucket: Math.floor(Math.random() * 5) + 1,       // 1–5
    watch_history_len: Math.floor(Math.random() * 200),        // 0–199
    subscription_tier: Math.floor(Math.random() * 3),          // 0–2
  };
}

// ---------------------------------------------------------------------------
// Scenario: SelectArm (hot path — 95% of traffic)
// ---------------------------------------------------------------------------

export function selectArm() {
  client.connect(ADDR, { plaintext: true, timeout: "5s" });

  const res = client.invoke(`${SERVICE}/SelectArm`, {
    experiment_id: randomExperimentId(),
    user_id: randomUserId(),
    context_features: randomContextFeatures(),
  });

  selectArmCount.add(1);
  selectArmErrors.add(res.status !== grpc.StatusOK);

  check(res, {
    "SelectArm: status OK": (r) => r.status === grpc.StatusOK,
    "SelectArm: has arm_id": (r) =>
      r.status === grpc.StatusOK && r.message && (r.message.armId || r.message.arm_id),
    "SelectArm: has probabilities": (r) =>
      r.status === grpc.StatusOK &&
      r.message &&
      (r.message.allArmProbabilities || r.message.all_arm_probabilities),
  });

  client.close();
}

// ---------------------------------------------------------------------------
// Scenario: Management RPCs (ExportAffinityScores + GetPolicySnapshot, 50/50)
// ---------------------------------------------------------------------------

export function managementRpc() {
  client.connect(ADDR, { plaintext: true, timeout: "10s" });

  const expId = randomExperimentId();
  let res;

  if (Math.random() < 0.5) {
    res = client.invoke(`${SERVICE}/ExportAffinityScores`, {
      experiment_id: expId,
    });
  } else {
    res = client.invoke(`${SERVICE}/GetPolicySnapshot`, {
      experiment_id: expId,
    });
  }

  mgmtCount.add(1);
  mgmtErrors.add(res.status !== grpc.StatusOK);

  check(res, {
    "Management: status OK": (r) => r.status === grpc.StatusOK,
  });

  client.close();
}

// ---------------------------------------------------------------------------
// Summary
// ---------------------------------------------------------------------------

export function handleSummary(data) {
  const get = (name, stat) => data.metrics[name]?.values?.[stat];

  const grpcP50 = get("grpc_req_duration", "p(50)")?.toFixed(2) || "N/A";
  const grpcP95 = get("grpc_req_duration", "p(95)")?.toFixed(2) || "N/A";
  const grpcP99 = get("grpc_req_duration", "p(99)")?.toFixed(2) || "N/A";
  const grpcMax = get("grpc_req_duration", "max")?.toFixed(2) || "N/A";
  const grpcTotal = get("grpc_req_duration", "count") || 0;

  const selectTotal = get("m4b_select_arm_total", "count") || 0;
  const selectErrRate = get("m4b_select_arm_error_rate", "rate") || 0;
  const mgmtTotal = get("m4b_mgmt_total", "count") || 0;
  const mgmtErrRate = get("m4b_mgmt_error_rate", "rate") || 0;

  const durationSecs = parseFloat(DURATION) || 60;
  const totalRps = grpcTotal / durationSecs;

  // SLA check
  const grpcP99Val = parseFloat(grpcP99);
  const slaPass = !isNaN(grpcP99Val) && grpcP99Val < 15.0;
  const errPass = selectErrRate < 0.001;
  const allPass = slaPass && errPass;

  const report = `
=============================================================
  M4b BANDIT POLICY SERVICE — LOAD TEST REPORT
=============================================================
  Target:          ${TARGET_RPS} rps sustained for ${DURATION}
  Server:          ${ADDR}
  Experiments:     ${EXPERIMENT_IDS.length} LinUCB cold-start policies

  --- SelectArm (${selectTotal} calls, 95% of traffic) ---
  p50 latency:     ${grpcP50} ms
  p95 latency:     ${grpcP95} ms
  p99 latency:     ${grpcP99} ms   (SLA: < 15ms)  ${slaPass ? "PASS" : "FAIL"}
  max latency:     ${grpcMax} ms
  Error rate:      ${(selectErrRate * 100).toFixed(3)}%   (SLA: < 0.1%)  ${errPass ? "PASS" : "FAIL"}

  --- Management RPCs (${mgmtTotal} calls, 5% of traffic) ---
  Error rate:      ${(mgmtErrRate * 100).toFixed(3)}%

  --- Overall gRPC ---
  Total requests:  ${grpcTotal}
  Achieved rps:    ${totalRps.toFixed(0)}
=============================================================
  RESULT: ${allPass ? "PASS — All SLAs met" : "FAIL — SLA violation detected"}
=============================================================
`;
  console.log(report);

  return {
    stdout: report,
    "loadtest_policy_results.json": JSON.stringify(
      {
        grpc_p50_ms: parseFloat(grpcP50) || null,
        grpc_p95_ms: parseFloat(grpcP95) || null,
        grpc_p99_ms: parseFloat(grpcP99) || null,
        grpc_max_ms: parseFloat(grpcMax) || null,
        select_arm_total: selectTotal,
        select_arm_error_rate: selectErrRate,
        mgmt_total: mgmtTotal,
        mgmt_error_rate: mgmtErrRate,
        total_rps: totalRps,
        sla_latency_pass: slaPass,
        sla_error_pass: errPass,
        all_pass: allPass,
      },
      null,
      2
    ),
  };
}
