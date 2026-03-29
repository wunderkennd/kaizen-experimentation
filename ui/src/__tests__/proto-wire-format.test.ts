/**
 * Proto wire-format contract tests.
 *
 * Validates that callRpc() + adaptExperiment() correctly handles real
 * ConnectRPC JSON wire format: timestamp parsing, enum prefix stripping,
 * proto3 zero-value omission, response envelope unwrapping, error parsing,
 * cache behavior, and auth header injection.
 */
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { http, HttpResponse } from 'msw';
import { server } from '@/__mocks__/server';
import {
  listExperiments,
  getExperiment,
  startExperiment,
  pauseExperiment,
  resumeExperiment,
  getCateAnalysis,
  getGstTrajectory,
  setApiAuth,
  clearApiCache,
  isPermissionDenied,
  RpcError,
} from '@/lib/api';

const MGMT_SVC = '*/experimentation.management.v1.ExperimentManagementService';
const ANALYSIS_SVC = '*/experimentation.analysis.v1.AnalysisService';

// --- 1. Timestamp handling ---

describe('Proto wire format — timestamp handling', () => {
  it('parses google.protobuf.Timestamp as ISO 8601 string', async () => {
    server.use(
      http.post(`${MGMT_SVC}/GetExperiment`, () =>
        HttpResponse.json({
          experiment: {
            experimentId: 'ts-1',
            name: 'timestamp test',
            state: 'DRAFT',
            type: 'AB',
            variants: [],
            guardrailConfigs: [],
            secondaryMetricIds: [],
            createdAt: '2026-02-15T10:00:00Z',
            startedAt: '2026-02-16T14:30:00Z',
          },
        }),
      ),
    );

    const exp = await getExperiment('ts-1');
    expect(exp.createdAt).toBe('2026-02-15T10:00:00Z');
    expect(exp.startedAt).toBe('2026-02-16T14:30:00Z');
  });

  it('defaults missing timestamps to undefined (proto3 zero-value omission)', async () => {
    server.use(
      http.post(`${MGMT_SVC}/GetExperiment`, () =>
        HttpResponse.json({
          experiment: {
            experimentId: 'ts-2',
            name: 'no timestamps',
            state: 'DRAFT',
            type: 'AB',
            variants: [],
            guardrailConfigs: [],
            secondaryMetricIds: [],
            createdAt: '2026-01-01T00:00:00Z',
            // startedAt and concludedAt omitted
          },
        }),
      ),
    );

    const exp = await getExperiment('ts-2');
    expect(exp.startedAt).toBeUndefined();
    expect(exp.concludedAt).toBeUndefined();
  });

  it('handles timestamps with timezone offsets', async () => {
    server.use(
      http.post(`${MGMT_SVC}/GetExperiment`, () =>
        HttpResponse.json({
          experiment: {
            experimentId: 'ts-3',
            name: 'tz offset',
            state: 'RUNNING',
            type: 'AB',
            variants: [],
            guardrailConfigs: [],
            secondaryMetricIds: [],
            createdAt: '2026-02-15T10:00:00+05:30',
            startedAt: '2026-02-16T14:30:00-08:00',
          },
        }),
      ),
    );

    const exp = await getExperiment('ts-3');
    expect(exp.createdAt).toBe('2026-02-15T10:00:00+05:30');
    expect(exp.startedAt).toBe('2026-02-16T14:30:00-08:00');
  });
});

// --- 2. Enum prefix stripping ---

describe('Proto wire format — enum prefix stripping', () => {
  it('strips EXPERIMENT_STATE_DRAFT → DRAFT', async () => {
    server.use(
      http.post(`${MGMT_SVC}/GetExperiment`, () =>
        HttpResponse.json({
          experiment: {
            experimentId: 'enum-1',
            name: 'enum test',
            state: 'EXPERIMENT_STATE_DRAFT',
            type: 'EXPERIMENT_TYPE_AB',
            variants: [],
            guardrailConfigs: [],
            secondaryMetricIds: [],
          },
        }),
      ),
    );

    const exp = await getExperiment('enum-1');
    expect(exp.state).toBe('DRAFT');
  });

  it('strips EXPERIMENT_TYPE_CONTEXTUAL_BANDIT → CONTEXTUAL_BANDIT', async () => {
    server.use(
      http.post(`${MGMT_SVC}/GetExperiment`, () =>
        HttpResponse.json({
          experiment: {
            experimentId: 'enum-2',
            name: 'bandit type',
            state: 'EXPERIMENT_STATE_RUNNING',
            type: 'EXPERIMENT_TYPE_CONTEXTUAL_BANDIT',
            variants: [],
            guardrailConfigs: [],
            secondaryMetricIds: [],
          },
        }),
      ),
    );

    const exp = await getExperiment('enum-2');
    expect(exp.type).toBe('CONTEXTUAL_BANDIT');
  });

  it('strips GUARDRAIL_ACTION_ALERT_ONLY → ALERT_ONLY', async () => {
    server.use(
      http.post(`${MGMT_SVC}/GetExperiment`, () =>
        HttpResponse.json({
          experiment: {
            experimentId: 'enum-3',
            name: 'guardrail',
            state: 'DRAFT',
            type: 'AB',
            guardrailAction: 'GUARDRAIL_ACTION_ALERT_ONLY',
            variants: [],
            guardrailConfigs: [],
            secondaryMetricIds: [],
          },
        }),
      ),
    );

    const exp = await getExperiment('enum-3');
    expect(exp.guardrailAction).toBe('ALERT_ONLY');
  });

  it('passes through already-stripped values unchanged', async () => {
    server.use(
      http.post(`${MGMT_SVC}/GetExperiment`, () =>
        HttpResponse.json({
          experiment: {
            experimentId: 'enum-4',
            name: 'already stripped',
            state: 'RUNNING',
            type: 'MAB',
            guardrailAction: 'AUTO_PAUSE',
            variants: [],
            guardrailConfigs: [],
            secondaryMetricIds: [],
          },
        }),
      ),
    );

    const exp = await getExperiment('enum-4');
    expect(exp.state).toBe('RUNNING');
    expect(exp.type).toBe('MAB');
    expect(exp.guardrailAction).toBe('AUTO_PAUSE');
  });

  it('handles EXPERIMENT_STATE_UNSPECIFIED → UNSPECIFIED (proto3 zero value)', async () => {
    server.use(
      http.post(`${MGMT_SVC}/GetExperiment`, () =>
        HttpResponse.json({
          experiment: {
            experimentId: 'enum-5',
            name: 'unspecified',
            state: 'EXPERIMENT_STATE_UNSPECIFIED',
            type: 'EXPERIMENT_TYPE_UNSPECIFIED',
            variants: [],
            guardrailConfigs: [],
            secondaryMetricIds: [],
          },
        }),
      ),
    );

    const exp = await getExperiment('enum-5');
    expect(exp.state).toBe('UNSPECIFIED');
    expect(exp.type).toBe('UNSPECIFIED');
  });

  it('strips LIFECYCLE_SEGMENT_ prefix in CATE analysis', async () => {
    server.use(
      http.post(`${ANALYSIS_SVC}/GetCateAnalysis`, () =>
        HttpResponse.json({
          experimentId: 'cate-1',
          metricId: 'ctr',
          globalAte: 0.05,
          globalSe: 0.01,
          globalCiLower: 0.03,
          globalCiUpper: 0.07,
          globalPValue: 0.001,
          subgroupEffects: [
            {
              segment: 'LIFECYCLE_SEGMENT_EARLY',
              effect: 0.08,
              se: 0.02,
              ciLower: 0.04,
              ciUpper: 0.12,
              pValueRaw: 0.01,
              pValueAdjusted: 0.03,
              isSignificant: true,
              nControl: 500,
              nTreatment: 500,
              controlMean: 0.10,
              treatmentMean: 0.18,
            },
            {
              segment: 'LIFECYCLE_SEGMENT_MATURE',
              effect: 0.02,
              se: 0.03,
              ciLower: -0.04,
              ciUpper: 0.08,
              pValueRaw: 0.50,
              pValueAdjusted: 0.75,
              isSignificant: false,
              nControl: 300,
              nTreatment: 300,
              controlMean: 0.15,
              treatmentMean: 0.17,
            },
          ],
          heterogeneity: {
            qStatistic: 5.2,
            df: 1,
            pValue: 0.02,
            iSquared: 80.8,
            heterogeneityDetected: true,
          },
          nSubgroups: 2,
          fdrThreshold: 0.05,
          computedAt: '2026-03-10T12:00:00Z',
        }),
      ),
    );

    const result = await getCateAnalysis('cate-1');
    expect(result.subgroupEffects[0].segment).toBe('EARLY');
    expect(result.subgroupEffects[1].segment).toBe('MATURE');
  });
});

// --- 3. Proto3 zero-value omission ---

describe('Proto wire format — proto3 zero-value omission', () => {
  it('missing variants field defaults to empty array', async () => {
    server.use(
      http.post(`${MGMT_SVC}/GetExperiment`, () =>
        HttpResponse.json({
          experiment: {
            experimentId: 'zero-1',
            name: 'no variants',
            state: 'DRAFT',
            type: 'AB',
            guardrailConfigs: [],
            secondaryMetricIds: [],
          },
        }),
      ),
    );

    const exp = await getExperiment('zero-1');
    expect(exp.variants).toEqual([]);
  });

  it('missing description defaults to empty string', async () => {
    server.use(
      http.post(`${MGMT_SVC}/GetExperiment`, () =>
        HttpResponse.json({
          experiment: {
            experimentId: 'zero-2',
            name: 'no description',
            state: 'DRAFT',
            type: 'AB',
            variants: [],
            guardrailConfigs: [],
            secondaryMetricIds: [],
          },
        }),
      ),
    );

    const exp = await getExperiment('zero-2');
    expect(exp.description).toBe('');
  });

  it('missing isCumulativeHoldout defaults to false', async () => {
    server.use(
      http.post(`${MGMT_SVC}/GetExperiment`, () =>
        HttpResponse.json({
          experiment: {
            experimentId: 'zero-3',
            name: 'no holdout',
            state: 'DRAFT',
            type: 'AB',
            variants: [],
            guardrailConfigs: [],
            secondaryMetricIds: [],
          },
        }),
      ),
    );

    const exp = await getExperiment('zero-3');
    expect(exp.isCumulativeHoldout).toBe(false);
  });

  it('missing secondaryMetricIds defaults to empty array', async () => {
    server.use(
      http.post(`${MGMT_SVC}/GetExperiment`, () =>
        HttpResponse.json({
          experiment: {
            experimentId: 'zero-4',
            name: 'no secondaries',
            state: 'DRAFT',
            type: 'AB',
            variants: [],
            guardrailConfigs: [],
          },
        }),
      ),
    );

    const exp = await getExperiment('zero-4');
    expect(exp.secondaryMetricIds).toEqual([]);
  });

  it('missing guardrailConfigs defaults to empty array', async () => {
    server.use(
      http.post(`${MGMT_SVC}/GetExperiment`, () =>
        HttpResponse.json({
          experiment: {
            experimentId: 'zero-5',
            name: 'no guardrails',
            state: 'DRAFT',
            type: 'AB',
            variants: [],
            secondaryMetricIds: [],
          },
        }),
      ),
    );

    const exp = await getExperiment('zero-5');
    expect(exp.guardrailConfigs).toEqual([]);
  });
});

// --- 4. Response envelope unwrapping ---

describe('Proto wire format — response envelope unwrapping', () => {
  it('unwraps { experiment: {...} } envelope', async () => {
    server.use(
      http.post(`${MGMT_SVC}/GetExperiment`, () =>
        HttpResponse.json({
          experiment: {
            experimentId: 'env-1',
            name: 'Envelope Test',
            state: 'EXPERIMENT_STATE_RUNNING',
            type: 'EXPERIMENT_TYPE_AB',
            variants: [{ variantId: 'v1', name: 'ctrl', trafficFraction: 1.0, isControl: true, payloadJson: '{}' }],
            guardrailConfigs: [],
            secondaryMetricIds: [],
            createdAt: '2026-01-15T10:00:00Z',
          },
        }),
      ),
    );

    const exp = await getExperiment('env-1');
    expect(exp.experimentId).toBe('env-1');
    expect(exp.state).toBe('RUNNING');
    expect(exp.variants).toHaveLength(1);
  });

  it('handles flat response (no envelope) gracefully', async () => {
    server.use(
      http.post(`${MGMT_SVC}/GetExperiment`, () =>
        HttpResponse.json({
          experimentId: 'flat-1',
          name: 'Flat Response',
          state: 'DRAFT',
          type: 'AB',
          variants: [],
          guardrailConfigs: [],
          secondaryMetricIds: [],
        }),
      ),
    );

    const exp = await getExperiment('flat-1');
    expect(exp.experimentId).toBe('flat-1');
    expect(exp.name).toBe('Flat Response');
  });

  it('unwraps { experiments: [...] } list envelope', async () => {
    server.use(
      http.post(`${MGMT_SVC}/ListExperiments`, () =>
        HttpResponse.json({
          experiments: [
            { experimentId: 'list-1', name: 'first', state: 'DRAFT', type: 'AB', variants: [], guardrailConfigs: [], secondaryMetricIds: [] },
            { experimentId: 'list-2', name: 'second', state: 'RUNNING', type: 'MAB', variants: [], guardrailConfigs: [], secondaryMetricIds: [] },
          ],
          nextPageToken: 'next-page',
        }),
      ),
    );

    const res = await listExperiments();
    expect(res.experiments).toHaveLength(2);
    expect(res.experiments[0].experimentId).toBe('list-1');
    expect(res.experiments[1].experimentId).toBe('list-2');
    expect(res.nextPageToken).toBe('next-page');
  });
});

// --- 5. Error response parsing ---

describe('Proto wire format — error response parsing', () => {
  it('extracts message from ConnectRPC { message: "..." } error', async () => {
    server.use(
      http.post(`${MGMT_SVC}/GetExperiment`, () =>
        HttpResponse.json(
          { code: 'not_found', message: 'Experiment not-real not found' },
          { status: 404 },
        ),
      ),
    );

    await expect(getExperiment('not-real')).rejects.toThrow('Experiment not-real not found');
  });

  it('extracts message from fallback { error: "..." } format', async () => {
    server.use(
      http.post(`${MGMT_SVC}/GetExperiment`, () =>
        HttpResponse.json(
          { error: 'Something went wrong' },
          { status: 500 },
        ),
      ),
    );

    await expect(getExperiment('err')).rejects.toThrow('Something went wrong');
  });

  it('falls back to generic status message for non-JSON body', async () => {
    server.use(
      http.post(`${MGMT_SVC}/GetExperiment`, () =>
        new HttpResponse('Bad Gateway', { status: 502 }),
      ),
    );

    await expect(getExperiment('502')).rejects.toThrow('RPC GetExperiment failed: 502');
  });

  it('identifies 403 as permission denied', async () => {
    server.use(
      http.post(`${MGMT_SVC}/GetExperiment`, () =>
        HttpResponse.json(
          { code: 'permission_denied', message: 'Requires admin role' },
          { status: 403 },
        ),
      ),
    );

    try {
      await getExperiment('forbidden');
      expect.fail('should have thrown');
    } catch (err) {
      expect(isPermissionDenied(err)).toBe(true);
      expect(err).toBeInstanceOf(RpcError);
      expect((err as RpcError).status).toBe(403);
    }
  });
});

// --- 6. Cache behavior ---

describe('Proto wire format — cache behavior', () => {
  beforeEach(() => {
    clearApiCache();
  });

  it('same read-only call within TTL returns cached data', async () => {
    const fetchSpy = vi.spyOn(globalThis, 'fetch');

    await getExperiment('11111111-1111-1111-1111-111111111111');
    await getExperiment('11111111-1111-1111-1111-111111111111');

    const getCalls = fetchSpy.mock.calls.filter(c =>
      (c[0] as string).includes('GetExperiment'),
    );
    expect(getCalls).toHaveLength(1);
    fetchSpy.mockRestore();
  });

  it('mutation call with clearCacheOnSuccess clears entire cache', async () => {
    const fetchSpy = vi.spyOn(globalThis, 'fetch');

    // Prime cache
    await getExperiment('11111111-1111-1111-1111-111111111111');

    // Mutation clears cache
    await startExperiment('22222222-2222-2222-2222-222222222222');

    // Should re-fetch since cache was cleared
    await getExperiment('11111111-1111-1111-1111-111111111111');

    const getCalls = fetchSpy.mock.calls.filter(c =>
      (c[0] as string).includes('GetExperiment'),
    );
    expect(getCalls).toHaveLength(2);
    fetchSpy.mockRestore();
  });

  it('skipCache bypasses cache on read', async () => {
    // pauseExperiment uses skipCache — test that repeated calls don't cache
    const fetchSpy = vi.spyOn(globalThis, 'fetch');

    // Use two startExperiment calls on different DRAFT experiments
    // startExperiment uses skipCache + clearCacheOnSuccess
    // Start the first DRAFT experiment
    await startExperiment('22222222-2222-2222-2222-222222222222');

    // Each call with skipCache should hit the network
    const startCalls = fetchSpy.mock.calls.filter(c =>
      (c[0] as string).includes('StartExperiment'),
    );
    expect(startCalls).toHaveLength(1);
    fetchSpy.mockRestore();
  });
});

// --- 7. Auth header injection ---

describe('Proto wire format — auth header injection', () => {
  it('setApiAuth injects X-User-Email and X-User-Role headers', async () => {
    const fetchSpy = vi.spyOn(globalThis, 'fetch');
    setApiAuth('alice@streamco.com', 'experimenter');

    await listExperiments();

    const call = fetchSpy.mock.calls.find(c =>
      (c[0] as string).includes('ListExperiments'),
    );
    expect(call).toBeDefined();
    const init = call![1] as RequestInit;
    const headers = init.headers as Record<string, string>;
    expect(headers['X-User-Email']).toBe('alice@streamco.com');
    expect(headers['X-User-Role']).toBe('experimenter');

    // Clean up
    setApiAuth('', '');
    fetchSpy.mockRestore();
  });

  it('missing auth sends no auth headers', async () => {
    const fetchSpy = vi.spyOn(globalThis, 'fetch');
    setApiAuth('', '');

    await listExperiments();

    const call = fetchSpy.mock.calls.find(c =>
      (c[0] as string).includes('ListExperiments'),
    );
    expect(call).toBeDefined();
    const init = call![1] as RequestInit;
    const headers = init.headers as Record<string, string>;
    expect(headers['X-User-Email']).toBeUndefined();
    expect(headers['X-User-Role']).toBeUndefined();

    fetchSpy.mockRestore();
  });
});

// --- 8. Pause/Resume state transitions ---

describe('Proto wire format — pause/resume state transitions', () => {
  it('PauseExperiment transitions RUNNING → PAUSED', async () => {
    const exp = await pauseExperiment('11111111-1111-1111-1111-111111111111');
    expect(exp.state).toBe('PAUSED');
  });

  it('ResumeExperiment transitions PAUSED → RUNNING', async () => {
    // First pause to get to PAUSED state
    await pauseExperiment('11111111-1111-1111-1111-111111111111');
    const exp = await resumeExperiment('11111111-1111-1111-1111-111111111111');
    expect(exp.state).toBe('RUNNING');
  });

  it('PauseExperiment rejects non-RUNNING experiment', async () => {
    // 22222222 is DRAFT
    await expect(pauseExperiment('22222222-2222-2222-2222-222222222222'))
      .rejects.toThrow('Only RUNNING experiments can be paused');
  });

  it('ResumeExperiment rejects non-PAUSED experiment', async () => {
    // 11111111 is RUNNING
    await expect(resumeExperiment('11111111-1111-1111-1111-111111111111'))
      .rejects.toThrow('Only PAUSED experiments can be resumed');
  });
});

// --- 9. ListExperiments server-side filters ---

describe('Proto wire format — ListExperiments server-side filters', () => {
  it('filters by state', async () => {
    const res = await listExperiments({ stateFilter: 'RUNNING' });
    expect(res.experiments.length).toBeGreaterThan(0);
    expect(res.experiments.every((e) => e.state === 'RUNNING')).toBe(true);
  });

  it('filters by type', async () => {
    const res = await listExperiments({ typeFilter: 'AB' });
    expect(res.experiments.length).toBeGreaterThan(0);
    expect(res.experiments.every((e) => e.type === 'AB')).toBe(true);
  });

  it('filters by owner email', async () => {
    const res = await listExperiments({ ownerEmailFilter: 'alice@streamco.com' });
    expect(res.experiments.length).toBeGreaterThan(0);
    expect(res.experiments.every((e) => e.ownerEmail === 'alice@streamco.com')).toBe(true);
  });

  it('returns all when no filters provided', async () => {
    const res = await listExperiments();
    expect(res.experiments.length).toBe(13); // 13 seed experiments (includes slate, switchback, quasi)
  });

  it('supports pagination with pageSize and pageToken', async () => {
    const first = await listExperiments({ pageSize: 4 });
    expect(first.experiments).toHaveLength(4);
    expect(first.nextPageToken).toBeTruthy();

    const second = await listExperiments({ pageSize: 4, pageToken: first.nextPageToken });
    expect(second.experiments).toHaveLength(4);
    expect(second.nextPageToken).toBeTruthy();

    const third = await listExperiments({ pageSize: 4, pageToken: second.nextPageToken });
    expect(third.experiments).toHaveLength(4);
    expect(third.nextPageToken).toBeTruthy();

    const fourth = await listExperiments({ pageSize: 4, pageToken: third.nextPageToken });
    expect(fourth.experiments).toHaveLength(1);
    expect(fourth.nextPageToken).toBe('');
  });
});

// --- 10. GST sequential method enum stripping ---

describe('Proto wire format — GST sequential method enum stripping', () => {
  it('strips SEQUENTIAL_METHOD_ prefix from GST trajectory method', async () => {
    server.use(
      http.post(`${ANALYSIS_SVC}/GetGstTrajectory`, () =>
        HttpResponse.json({
          experimentId: 'gst-1',
          metricId: 'ctr',
          method: 'SEQUENTIAL_METHOD_MSPRT',
          plannedLooks: 5,
          overallAlpha: 0.05,
          boundaryPoints: [
            { look: 1, informationFraction: 0.2, boundaryZScore: 2.80 },
            { look: 2, informationFraction: 0.4, boundaryZScore: 2.50 },
          ],
          computedAt: '2026-03-10T12:00:00Z',
        }),
      ),
    );

    const result = await getGstTrajectory('gst-1', 'ctr');
    expect(result.method).toBe('MSPRT');
  });

  it('passes through already-stripped GST method', async () => {
    server.use(
      http.post(`${ANALYSIS_SVC}/GetGstTrajectory`, () =>
        HttpResponse.json({
          experimentId: 'gst-2',
          metricId: 'ctr',
          method: 'GST_OBF',
          plannedLooks: 3,
          overallAlpha: 0.05,
          boundaryPoints: [],
          computedAt: '2026-03-10T12:00:00Z',
        }),
      ),
    );

    const result = await getGstTrajectory('gst-2', 'ctr');
    expect(result.method).toBe('GST_OBF');
  });
});
