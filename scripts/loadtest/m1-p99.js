// M1 Assignment service p99 smoke test (k6).
//
// Two use-cases share this script:
//   1. ADR-031 §4.1 pilot vs baseline comparison — same script, TWO runs
//      pointed at ports 50051 (tonic) and 50161 (Connect). Compare p99 to
//      confirm the pilot stays within ±10% of the tonic baseline.
//   2. #500 M1/M7 Cloud Run smoke test — one 60s run against the deployed
//      M1 URL, asserting p99 < 5ms per the SLA.
//
// Environment:
//   TARGET_URL     required. Full base URL (e.g. http://127.0.0.1:50051 or
//                  https://m1.kaizen.dev/). Protocol chosen automatically:
//                  ports 50051/50052/... => gRPC; others (Cloud Run, 50161)
//                  => Connect over HTTPS. Override with PROTOCOL=grpc|connect.
//   PROTOCOL       optional. grpc | connect. Auto-detected from port if unset.
//   DURATION       optional. Default 60s. Passed to k6 stage.
//   VUS            optional. Default 20. Concurrent virtual users.
//   P99_TARGET_MS  optional. Default 5. Threshold for the p99 assertion.
//   CONFIG_PATH    optional. Path to a JSON file with { experimentIds: [], slateIds: [] }.
//                  When set, the script draws exp/slate IDs from the file instead
//                  of the built-in dev/config.json defaults.
//
// Run examples:
//   # Local tonic baseline (dev)
//   TARGET_URL=http://127.0.0.1:50051 k6 run scripts/loadtest/m1-p99.js
//
//   # Local Connect pilot (dev, --features connectrpc)
//   TARGET_URL=http://127.0.0.1:50161 PROTOCOL=connect k6 run scripts/loadtest/m1-p99.js
//
//   # Cloud Run smoke (#500)
//   TARGET_URL=https://m1-assignment.kaizen.dev DURATION=60s P99_TARGET_MS=5 \
//     k6 run scripts/loadtest/m1-p99.js

import http from 'k6/http';
import grpc from 'k6/net/grpc';
import { check, sleep } from 'k6';
import { Trend, Rate } from 'k6/metrics';

// ---- Config resolution ---------------------------------------------------

const TARGET_URL = __ENV.TARGET_URL;
if (!TARGET_URL) {
  throw new Error('TARGET_URL is required (e.g. http://127.0.0.1:50051)');
}

const DURATION = __ENV.DURATION || '60s';
const VUS = parseInt(__ENV.VUS || '20', 10);
const P99_TARGET_MS = parseFloat(__ENV.P99_TARGET_MS || '5');

function detectProtocol() {
  if (__ENV.PROTOCOL) return __ENV.PROTOCOL;
  // gRPC (tonic) listens on the 5005x range in dev; anything else (Connect
  // dev port 50161, Cloud Run, etc.) speaks Connect over HTTPS.
  const m = TARGET_URL.match(/:(\d+)(\/|$)/);
  if (m && /^5005[0-9]$/.test(m[1])) return 'grpc';
  return 'connect';
}
const PROTOCOL = detectProtocol();

// Test corpus — matches dev/config.json experiment IDs; override via CONFIG_PATH.
const DEFAULT_CORPUS = {
  experimentIds: ['exp_dev_001', 'exp_dev_002', 'exp_dev_004', 'exp_dev_slate_001'],
  slateIds: ['exp_dev_slate_001'],
  interleavedIds: ['exp_dev_004'],
};
const CORPUS = __ENV.CONFIG_PATH
  ? JSON.parse(open(__ENV.CONFIG_PATH))
  : DEFAULT_CORPUS;

// ---- gRPC client bootstrap (baseline path) -------------------------------

const grpcClient = new grpc.Client();
if (PROTOCOL === 'grpc') {
  // Proto files must be reachable from k6's --include-system-env flag or
  // shipped alongside this script. Wire them at test-invocation time so k6
  // doesn't fail-load when running against Connect targets.
  grpcClient.load(
    ['../../proto'],
    'experimentation/assignment/v1/assignment.proto',
  );
}

// ---- Metrics -------------------------------------------------------------

const rpcLatency = new Trend('rpc_latency_ms', true);
const rpcErrors = new Rate('rpc_errors');

// ---- Options: single 60s stage, threshold on p99 -------------------------

export const options = {
  scenarios: {
    steady: {
      executor: 'constant-vus',
      vus: VUS,
      duration: DURATION,
    },
  },
  thresholds: {
    // Global p99 gate — the load-bearing assertion for #500 and ADR-031.
    'rpc_latency_ms': [`p(99)<${P99_TARGET_MS}`],
    'rpc_errors': ['rate<0.001'],
  },
};

// ---- VU lifecycle --------------------------------------------------------

export function setup() {
  if (PROTOCOL === 'grpc') {
    // Verify server is up before starting the load. k6 doesn't fail-fast on
    // connect errors inside the VU loop; explicit ping avoids a 60s window
    // of noise if the URL is wrong.
    const url = TARGET_URL.replace(/^https?:\/\//, '');
    grpcClient.connect(url, { plaintext: TARGET_URL.startsWith('http://') });
    const resp = grpcClient.invoke('grpc.health.v1.Health/Check', {});
    if (resp.status !== grpc.StatusOK) {
      throw new Error(`grpc.health.v1.Health/Check failed: status=${resp.status}`);
    }
    grpcClient.close();
  }
  return { url: TARGET_URL, protocol: PROTOCOL };
}

// ---- Request builders ----------------------------------------------------

function pick(arr) { return arr[Math.floor(Math.random() * arr.length)]; }

function randUser() {
  return `loadtest-user-${Math.floor(Math.random() * 1_000_000)}`;
}

function assignmentReq() {
  return {
    userId: randUser(),
    experimentId: pick(CORPUS.experimentIds),
    sessionId: `sess-${__VU}-${__ITER}`,
  };
}

function assignmentsReq() {
  return { userId: randUser(), sessionId: `sess-${__VU}-${__ITER}` };
}

function slateReq() {
  return {
    userId: randUser(),
    experimentId: pick(CORPUS.slateIds),
    candidateItemIds: ['i1', 'i2', 'i3', 'i4', 'i5', 'i6'],
  };
}

function interleavedReq() {
  return {
    experimentId: pick(CORPUS.interleavedIds),
    userId: randUser(),
    algorithmLists: {
      algo_a: { itemIds: ['a1', 'a2', 'a3'] },
      algo_b: { itemIds: ['b1', 'b2', 'b3'] },
    },
  };
}

const RPCS = [
  { name: 'GetAssignment',       method: 'experimentation.assignment.v1.AssignmentService/GetAssignment',       body: assignmentReq },
  { name: 'GetAssignments',      method: 'experimentation.assignment.v1.AssignmentService/GetAssignments',      body: assignmentsReq },
  { name: 'GetSlateAssignment',  method: 'experimentation.assignment.v1.AssignmentService/GetSlateAssignment',  body: slateReq },
  { name: 'GetInterleavedList',  method: 'experimentation.assignment.v1.AssignmentService/GetInterleavedList',  body: interleavedReq },
];

// ---- The hot loop --------------------------------------------------------

function invokeConnect(url, method, body) {
  // Connect unary over HTTP: POST /<method> with JSON body.
  const resp = http.post(`${url}/${method}`, JSON.stringify(body), {
    headers: {
      'Content-Type': 'application/json',
      'Connect-Protocol-Version': '1',
    },
  });
  return { ok: resp.status === 200, latency: resp.timings.duration };
}

function invokeGrpc(method, body) {
  // grpc.Client is connect-per-invoke in k6's default mode; the setup()
  // connect above only proved reachability.
  const resp = grpcClient.invoke(method, body);
  return { ok: resp.status === grpc.StatusOK, latency: resp.rt };
}

// Per-VU state for gRPC connection reuse. k6 gives each VU its own JS
// context, so a module-level bool is per-VU, not global.
let grpcConnected = false;

export default function (data) {
  if (data.protocol === 'grpc' && !grpcConnected) {
    const url = data.url.replace(/^https?:\/\//, '');
    grpcClient.connect(url, { plaintext: data.url.startsWith('http://') });
    grpcConnected = true;
  }

  const rpc = pick(RPCS);
  const body = rpc.body();
  const result = data.protocol === 'grpc'
    ? invokeGrpc(rpc.method, body)
    : invokeConnect(data.url, rpc.method, body);

  rpcLatency.add(result.latency, { rpc: rpc.name, protocol: data.protocol });
  rpcErrors.add(!result.ok, { rpc: rpc.name });

  check(result, { 'ok': (r) => r.ok });
  sleep(0.01); // 100 rps per VU ceiling; global throughput ~= VUS * 100
}

// No explicit teardown — grpcClient is per-VU (module scope in k6's per-VU
// context) and k6 closes its connections at VU shutdown automatically. A
// teardown() hook here runs outside any VU context and can't reach the
// per-VU flag anyway.
