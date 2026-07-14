import { NextRequest, NextResponse } from 'next/server';

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
