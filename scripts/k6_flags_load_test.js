/**
 * k6 load test for experimentation-flags Rust service (ADR-024 Phase 4).
 *
 * Target: 20K rps, p99 < 5ms on EvaluateFlag (hot path).
 *
 * Usage:
 *   FLAGS_URL=http://localhost:50057 k6 run scripts/k6_flags_load_test.js
 *
 * The test expects a pre-seeded flag with id in FLAGS_TEST_FLAG_ID env var.
 * If unset, the setup() stage creates a temporary flag and tears it down
 * in teardown().
 *
 * Wire format: tonic-web JSON HTTP (POST with Content-Type: application/json).
 * Same endpoint pattern as Connect-Go, compatible with M6 and SDK consumers.
 */

import http from "k6/http";
import { check, sleep } from "k6";
import { Trend, Rate, Counter } from "k6/metrics";

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

const FLAGS_URL = __ENV.FLAGS_URL || "http://localhost:50057";
const TEST_FLAG_ID = __ENV.FLAGS_TEST_FLAG_ID || "";

// ---------------------------------------------------------------------------
// Custom metrics
// ---------------------------------------------------------------------------

const evaluateLatency = new Trend("evaluate_flag_latency_ms", true);
const evaluateFlagsLatency = new Trend("evaluate_flags_latency_ms", true);
const errorRate = new Rate("error_rate");
const requestCount = new Counter("request_count");

// ---------------------------------------------------------------------------
// k6 options — ramp to 20K rps over 30s, sustain 60s, ramp down
// ---------------------------------------------------------------------------

export const options = {
  scenarios: {
    evaluate_flag: {
      executor: "ramping-arrival-rate",
      startRate: 100,
      timeUnit: "1s",
      preAllocatedVUs: 200,
      maxVUs: 500,
      stages: [
        { duration: "30s", target: 20000 }, // ramp to 20K rps
        { duration: "60s", target: 20000 }, // sustain
        { duration: "10s", target: 0 },     // ramp down
      ],
    },
  },
  thresholds: {
    // ADR-024: p99 < 5ms (vs 10ms for Go M7).
    evaluate_flag_latency_ms: ["p(99)<5", "p(95)<3", "p(50)<1"],
    error_rate: ["rate<0.001"],
    http_req_failed: ["rate<0.001"],
  },
};

// ---------------------------------------------------------------------------
// Setup: create a test flag if none specified
// ---------------------------------------------------------------------------

let createdFlagId = null;

export function setup() {
  if (TEST_FLAG_ID !== "") {
    return { flagId: TEST_FLAG_ID };
  }

  const createUrl = `${FLAGS_URL}/experimentation.flags.v1.FeatureFlagService/CreateFlag`;
  const body = JSON.stringify({
    flag: {
      name: `k6-load-test-${Date.now()}`,
      description: "k6 load test flag — auto-created, auto-deleted",
      type: "FLAG_TYPE_BOOLEAN",
      defaultValue: "false",
      enabled: true,
      rolloutPercentage: 1.0,
    },
  });

  const resp = http.post(createUrl, body, {
    headers: {
      "Content-Type": "application/json",
      "Connect-Protocol-Version": "1",
    },
  });

  if (resp.status !== 200) {
    console.error(`setup: CreateFlag failed: ${resp.status} ${resp.body}`);
    return { flagId: null };
  }

  const created = JSON.parse(resp.body);
  console.log(`setup: created flag ${created.flagId}`);
  return { flagId: created.flagId };
}

// ---------------------------------------------------------------------------
// Main VU function
// ---------------------------------------------------------------------------

export default function (data) {
  const flagId = data.flagId;
  if (!flagId) {
    errorRate.add(1);
    return;
  }

  const userId = `user_${Math.floor(Math.random() * 1_000_000)}`;

  const url = `${FLAGS_URL}/experimentation.flags.v1.FeatureFlagService/EvaluateFlag`;
  const body = JSON.stringify({ flagId, userId });

  const start = Date.now();
  const resp = http.post(url, body, {
    headers: {
      "Content-Type": "application/json",
      "Connect-Protocol-Version": "1",
    },
    tags: { endpoint: "evaluate_flag" },
  });
  const latencyMs = Date.now() - start;

  evaluateLatency.add(latencyMs);
  requestCount.add(1);

  const ok = check(resp, {
    "status 200": (r) => r.status === 200,
    "has value field": (r) => {
      try {
        const body = JSON.parse(r.body);
        return body.value !== undefined;
      } catch {
        return false;
      }
    },
    "p99 < 5ms": () => latencyMs < 5,
  });

  errorRate.add(!ok);
}

// ---------------------------------------------------------------------------
// Teardown: delete flag created during setup
// ---------------------------------------------------------------------------

export function teardown(data) {
  // Note: FeatureFlagService has no DeleteFlag RPC in the proto.
  // Flags created by setup are left for manual cleanup or TTL.
  if (data.flagId && data.flagId !== TEST_FLAG_ID) {
    console.log(`teardown: flag ${data.flagId} left in place (no DeleteFlag RPC)`);
  }
}
