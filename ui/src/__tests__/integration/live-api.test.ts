/**
 * Integration tests for live Agent-5 API.
 *
 * Skipped by default. Run with:
 *   LIVE_API=true npx vitest run src/__tests__/integration/
 *
 * Requires Agent-5 management service running at http://localhost:50055.
 * Start it with: just dev
 */
import { describe, it, expect } from 'vitest';

const LIVE = process.env.LIVE_API === 'true';
const describeIf = LIVE ? describe : describe.skip;

const MGMT_BASE = 'http://localhost:50055';
const MGMT_SVC = 'experimentation.management.v1.ExperimentManagementService';

async function rpc<Res>(method: string, body: Record<string, unknown> = {}): Promise<{ status: number; data: Res }> {
  const res = await fetch(`${MGMT_BASE}/${MGMT_SVC}/${method}`, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      'X-User-Email': 'test@streamco.com',
      'X-User-Role': 'admin',
    },
    body: JSON.stringify(body),
  });
  const data = await res.json().catch(() => ({})) as Res;
  return { status: res.status, data };
}

describeIf('Live API — Management Service', () => {
  it('ListExperiments returns valid response with experiments array', async () => {
    const { status, data } = await rpc<{ experiments?: unknown[] }>('ListExperiments');
    expect(status).toBe(200);
    expect(data).toHaveProperty('experiments');
    expect(Array.isArray(data.experiments)).toBe(true);
  });

  it('GetExperiment with seed ID returns experiment matching proto shape', async () => {
    // First get an experiment ID from the list
    const { data: listData } = await rpc<{ experiments?: Array<{ experimentId: string }> }>('ListExperiments');
    const firstId = listData.experiments?.[0]?.experimentId;
    if (!firstId) {
      console.warn('No experiments in database — skipping GetExperiment test');
      return;
    }

    const { status, data } = await rpc<Record<string, unknown>>('GetExperiment', { experimentId: firstId });
    expect(status).toBe(200);

    // Check required proto fields are present (either at top level or nested under experiment)
    const exp = (data as { experiment?: Record<string, unknown> }).experiment || data;
    expect(exp).toHaveProperty('experimentId');
    expect(exp).toHaveProperty('name');
    expect(exp).toHaveProperty('state');
    expect(exp).toHaveProperty('type');
  });

  it('GetExperiment with non-existent ID returns error', async () => {
    const { status } = await rpc<Record<string, unknown>>('GetExperiment', {
      experimentId: '00000000-0000-0000-0000-000000000000',
    });
    // ConnectRPC typically returns 404 or 400 for not found
    expect(status).toBeGreaterThanOrEqual(400);
  });

  it('response uses camelCase field names', async () => {
    const { data } = await rpc<{ experiments?: Array<Record<string, unknown>> }>('ListExperiments');
    if (!data.experiments?.length) return;

    const exp = data.experiments[0];
    // Proto JSON uses camelCase by default
    expect(exp).toHaveProperty('experimentId');
    // Should NOT have snake_case
    expect(exp).not.toHaveProperty('experiment_id');
  });

  it('enum values use prefixed format', async () => {
    const { data } = await rpc<{ experiments?: Array<{ state?: string; type?: string }> }>('ListExperiments');
    if (!data.experiments?.length) return;

    const exp = data.experiments[0];
    if (exp.state) {
      expect(exp.state).toMatch(/^EXPERIMENT_STATE_/);
    }
    if (exp.type) {
      expect(exp.type).toMatch(/^EXPERIMENT_TYPE_/);
    }
  });

  it('auth headers are forwarded', async () => {
    // Making a request with auth headers should not cause 401
    const { status } = await rpc<unknown>('ListExperiments');
    expect(status).not.toBe(401);
  });
});
