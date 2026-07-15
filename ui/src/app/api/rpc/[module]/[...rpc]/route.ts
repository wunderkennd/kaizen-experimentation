import { NextRequest, NextResponse } from 'next/server';
import { ConnectError, createPromiseClient, type Transport } from '@connectrpc/connect';
import { createGrpcTransport } from '@connectrpc/connect-node';
import type { MethodInfo, ServiceType } from '@bufbuild/protobuf';
import { MethodKind } from '@bufbuild/protobuf';
import { FeatureFlagService } from '../../../../../../../gen/ts/experimentation/flags/v1/flags_service_connect';

// Runtime BFF proxy: /api/rpc/<module>/<pkg.Service>/<Method> → backend.
//
// This replaces the next.config.js rewrites() that previously served these
// paths. With `output: 'standalone'`, rewrites are resolved at BUILD time
// and frozen into the routes manifest — the BACKEND_*_URL values present in
// the deployed container were never read, so every destination pointed at
// the build machine's localhost. A route handler reads process.env on each
// request, so one image works in every environment.
export const runtime = 'nodejs';
export const dynamic = 'force-dynamic';

// Same defaults the deleted rewrites used, so `next dev` against locally
// running services keeps working without any env setup.
const DEV_DEFAULTS: Record<string, string> = {
  management: 'http://localhost:50055',
  metrics: 'http://localhost:50056',
  analysis: 'http://localhost:50053',
  bandit: 'http://localhost:50054',
  flags: 'http://localhost:50057',
  assignment: 'http://localhost:50051',
};

// ---------------------------------------------------------------------------
// TEMPORARY gRPC bridge — delete when connect-rust lands (#758).
//
// M7 Flags is tonic-only: it speaks native gRPC, not the Connect-JSON the
// browser client sends, so a byte-level pass-through gets HTTP 400 back.
// For modules listed here the BFF instead decodes the Connect-JSON request
// with the generated protobuf-es schema and re-issues it over the gRPC
// protocol (h2c prior knowledge, which tonic serves on its plaintext port).
//
// This is an interim shim, NOT the end state. The proper fix is adopting
// the ADR-031 `connectrpc` runtime in M7 (and later M4a/M4b/M1), which
// serves Connect + gRPC on one port and makes this bridge — and the JSON
// decode/re-encode it pays per request — deletable. Tracked in #758;
// fleet-wide connect-rust adoption is gated on the #645 pilot decision.
// When M7 speaks Connect natively, remove its entry below and the plain
// pass-through takes over unchanged.
// ---------------------------------------------------------------------------
const GRPC_BRIDGED: Record<string, ServiceType> = {
  flags: FeatureFlagService,
};

// gRPC transports hold HTTP/2 sessions worth reusing across requests.
const grpcTransports = new Map<string, Transport>();

function grpcTransportFor(baseUrl: string): Transport {
  let t = grpcTransports.get(baseUrl);
  if (!t) {
    t = createGrpcTransport({ baseUrl, httpVersion: '2' });
    grpcTransports.set(baseUrl, t);
  }
  return t;
}

const CODE_NAMES: Record<number, string> = {
  1: 'canceled', 2: 'unknown', 3: 'invalid_argument', 4: 'deadline_exceeded',
  5: 'not_found', 6: 'already_exists', 7: 'permission_denied', 8: 'resource_exhausted',
  9: 'failed_precondition', 10: 'aborted', 11: 'out_of_range', 12: 'unimplemented',
  13: 'internal', 14: 'unavailable', 15: 'data_loss', 16: 'unauthenticated',
};

// Connect error code → HTTP status, close to the Connect protocol's own
// unary mapping. The UI's error handling only reads status + message.
const CODE_TO_HTTP: Record<number, number> = {
  1: 408, 2: 500, 3: 400, 4: 408, 5: 404, 6: 409, 7: 403, 8: 429,
  9: 412, 10: 409, 11: 400, 12: 501, 13: 500, 14: 503, 15: 500, 16: 401,
};

/** Headers forwarded to bridged gRPC backends (auth/RBAC parity with M5). */
const BRIDGE_FORWARD_HEADERS = ['x-user-email', 'x-user-role', 'authorization'];

async function bridgeGrpc(
  req: NextRequest,
  service: ServiceType,
  rpc: string[],
  base: string,
): Promise<NextResponse> {
  const [serviceName, methodName] = rpc;
  if (rpc.length !== 2 || serviceName !== service.typeName) {
    return NextResponse.json(
      { code: 'unimplemented', message: `unknown RPC path /${rpc.join('/')} for bridged module` },
      { status: 404 },
    );
  }

  const entry = (Object.entries(service.methods) as Array<[string, MethodInfo]>).find(
    ([, m]) => m.name === methodName,
  );
  if (!entry || entry[1].kind !== MethodKind.Unary) {
    return NextResponse.json(
      { code: 'unimplemented', message: `method ${methodName} not found or not unary` },
      { status: 404 },
    );
  }
  const [localName, method] = entry;

  let requestJson: unknown = {};
  try {
    const raw = await req.text();
    requestJson = raw.length > 0 ? JSON.parse(raw) : {};
  } catch {
    return NextResponse.json(
      { code: 'invalid_argument', message: 'request body is not valid JSON' },
      { status: 400 },
    );
  }

  const headers = new Headers();
  for (const name of BRIDGE_FORWARD_HEADERS) {
    const v = req.headers.get(name);
    if (v) headers.set(name, v);
  }

  try {
    const request = method.I.fromJson(requestJson as never, { ignoreUnknownFields: true });
    const client = createPromiseClient(service, grpcTransportFor(base)) as unknown as Record<
      string,
      (r: unknown, o: { headers: Headers }) => Promise<{ toJson: () => unknown }>
    >;
    const response = await client[localName](request, { headers });
    return NextResponse.json(response.toJson() ?? {});
  } catch (err) {
    const ce = ConnectError.from(err);
    return NextResponse.json(
      { code: CODE_NAMES[ce.code] ?? 'unknown', message: ce.rawMessage },
      { status: CODE_TO_HTTP[ce.code] ?? 500 },
    );
  }
}

function backendFor(module: string): string | undefined {
  const fromEnv = process.env[`BACKEND_${module.toUpperCase()}_URL`];
  return fromEnv || DEV_DEFAULTS[module];
}

// Hop-by-hop headers (RFC 9110 §7.6.1) plus fields the proxy must own.
const STRIP_REQUEST_HEADERS = new Set([
  'host',
  'connection',
  'keep-alive',
  'transfer-encoding',
  'upgrade',
  'te',
  'trailer',
  'proxy-authenticate',
  'proxy-authorization',
  'content-length',
  'accept-encoding',
]);

const STRIP_RESPONSE_HEADERS = new Set([
  'connection',
  'keep-alive',
  'transfer-encoding',
  'content-encoding',
  'content-length',
]);

interface RouteParams {
  params: { module: string; rpc: string[] };
}

async function proxy(req: NextRequest, { params }: RouteParams): Promise<NextResponse> {
  const base = backendFor(params.module);
  if (!base) {
    return NextResponse.json(
      { code: 'unimplemented', message: `no backend configured for module "${params.module}"` },
      { status: 502 },
    );
  }

  // tonic-only backends can't parse Connect-JSON — translate to gRPC.
  const bridged = GRPC_BRIDGED[params.module];
  if (bridged) {
    return bridgeGrpc(req, bridged, params.rpc, base);
  }

  const target = `${base.replace(/\/+$/, '')}/${params.rpc.map(encodeURIComponent).join('/')}${req.nextUrl.search}`;

  const headers = new Headers();
  req.headers.forEach((value, key) => {
    if (!STRIP_REQUEST_HEADERS.has(key.toLowerCase())) headers.set(key, value);
  });

  let upstream: Response;
  try {
    upstream = await fetch(target, {
      method: req.method,
      headers,
      // All UI RPCs are unary; buffering keeps us off duplex-stream quirks.
      body: req.method === 'GET' || req.method === 'HEAD' ? undefined : await req.arrayBuffer(),
      cache: 'no-store',
      redirect: 'manual',
    });
  } catch {
    return NextResponse.json(
      { code: 'unavailable', message: `backend for "${params.module}" unreachable` },
      { status: 502 },
    );
  }

  const responseHeaders = new Headers();
  upstream.headers.forEach((value, key) => {
    if (!STRIP_RESPONSE_HEADERS.has(key.toLowerCase())) responseHeaders.set(key, value);
  });

  return new NextResponse(upstream.body, {
    status: upstream.status,
    headers: responseHeaders,
  });
}

export { proxy as GET, proxy as POST };
