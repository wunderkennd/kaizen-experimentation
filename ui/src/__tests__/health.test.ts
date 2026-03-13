import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { checkHealth } from '@/lib/health';
import { server } from '@/__mocks__/server';
import { http, HttpResponse } from 'msw';

const MGMT_URL = 'http://localhost:50055';
const MGMT_SVC = 'experimentation.management.v1.ExperimentManagementService';

describe('checkHealth', () => {
  it('returns healthy when management service responds 200', async () => {
    // Default MSW handler returns 200
    const result = await checkHealth();
    expect(result.allHealthy).toBe(true);
    expect(result.services).toHaveLength(1);
    expect(result.services[0].name).toBe('Management');
    expect(result.services[0].healthy).toBe(true);
    expect(result.services[0].latencyMs).toBeGreaterThanOrEqual(0);
    expect(result.checkedAt).toBeTruthy();
  });

  it('returns unhealthy on network error', async () => {
    server.use(
      http.post(`${MGMT_URL}/${MGMT_SVC}/ListExperiments`, () => {
        return HttpResponse.error();
      }),
    );

    const result = await checkHealth();
    expect(result.allHealthy).toBe(false);
    expect(result.services[0].healthy).toBe(false);
    expect(result.services[0].error).toBeTruthy();
    expect(result.services[0].latencyMs).toBeNull();
  });

  it('returns unhealthy on 500 server error', async () => {
    server.use(
      http.post(`${MGMT_URL}/${MGMT_SVC}/ListExperiments`, () => {
        return HttpResponse.json({ error: 'internal error' }, { status: 500 });
      }),
    );

    const result = await checkHealth();
    expect(result.allHealthy).toBe(false);
    expect(result.services[0].healthy).toBe(false);
  });

  it('returns healthy on 403 (service reachable but auth error)', async () => {
    server.use(
      http.post(`${MGMT_URL}/${MGMT_SVC}/ListExperiments`, () => {
        return HttpResponse.json({ error: 'forbidden' }, { status: 403 });
      }),
    );

    const result = await checkHealth();
    expect(result.allHealthy).toBe(true);
    expect(result.services[0].healthy).toBe(true);
    expect(result.services[0].latencyMs).toBeGreaterThanOrEqual(0);
  });

  it('includes checkedAt as ISO timestamp', async () => {
    const before = new Date().toISOString();
    const result = await checkHealth();
    const after = new Date().toISOString();
    expect(result.checkedAt >= before).toBe(true);
    expect(result.checkedAt <= after).toBe(true);
  });
});
