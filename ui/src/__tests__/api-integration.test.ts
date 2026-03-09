/**
 * Integration contract tests for the API client.
 *
 * These tests verify the wire-format adaptation between ConnectRPC
 * proto JSON (prefixed enums, nested wrappers) and our local types.
 * They run against the MSW mock server, validating:
 *   1. Proto enum prefix stripping (EXPERIMENT_STATE_DRAFT → DRAFT)
 *   2. ConnectRPC error response parsing
 *   3. Response envelope unwrapping ({ experiment: {...} } → Experiment)
 *   4. Default/fallback values for missing fields
 */
import { describe, it, expect } from 'vitest';
import { http, HttpResponse } from 'msw';
import { server } from '@/__mocks__/server';
import {
  listExperiments,
  getExperiment,
  createExperiment,
  startExperiment,
  concludeExperiment,
  archiveExperiment,
  getQueryLog,
  exportNotebook,
  getAnalysisResult,
  getNoveltyAnalysis,
  getInterferenceAnalysis,
  getInterleavingAnalysis,
  getBanditDashboard,
} from '@/lib/api';

const MGMT_SVC = '*/experimentation.management.v1.ExperimentManagementService';
const ANALYSIS_SVC = '*/experimentation.analysis.v1.AnalysisService';
const METRICS_SVC = '*/experimentation.metrics.v1.MetricComputationService';
const BANDIT_SVC = '*/experimentation.bandit.v1.BanditPolicyService';

describe('API contract — enum prefix stripping', () => {
  it('strips EXPERIMENT_STATE_ prefix from proto enums', async () => {
    server.use(
      http.post(`${MGMT_SVC}/GetExperiment`, () =>
        HttpResponse.json({
          experiment: {
            experimentId: 'test-1',
            name: 'test',
            state: 'EXPERIMENT_STATE_RUNNING',
            type: 'EXPERIMENT_TYPE_AB',
            guardrailAction: 'GUARDRAIL_ACTION_ALERT_ONLY',
            variants: [],
            guardrailConfigs: [],
            secondaryMetricIds: [],
          },
        }),
      ),
    );

    const exp = await getExperiment('test-1');
    expect(exp.state).toBe('RUNNING');
    expect(exp.type).toBe('AB');
    expect(exp.guardrailAction).toBe('ALERT_ONLY');
  });

  it('passes through already-stripped enum values', async () => {
    server.use(
      http.post(`${MGMT_SVC}/GetExperiment`, () =>
        HttpResponse.json({
          experiment: {
            experimentId: 'test-2',
            name: 'test',
            state: 'DRAFT',
            type: 'MAB',
            guardrailAction: 'AUTO_PAUSE',
            variants: [],
            guardrailConfigs: [],
            secondaryMetricIds: [],
          },
        }),
      ),
    );

    const exp = await getExperiment('test-2');
    expect(exp.state).toBe('DRAFT');
    expect(exp.type).toBe('MAB');
    expect(exp.guardrailAction).toBe('AUTO_PAUSE');
  });

  it('strips prefixes for all experiment states in list response', async () => {
    server.use(
      http.post(`${MGMT_SVC}/ListExperiments`, () =>
        HttpResponse.json({
          experiments: [
            { experimentId: '1', name: 'a', state: 'EXPERIMENT_STATE_DRAFT', type: 'EXPERIMENT_TYPE_AB', variants: [], guardrailConfigs: [], secondaryMetricIds: [] },
            { experimentId: '2', name: 'b', state: 'EXPERIMENT_STATE_STARTING', type: 'EXPERIMENT_TYPE_MVT', variants: [], guardrailConfigs: [], secondaryMetricIds: [] },
            { experimentId: '3', name: 'c', state: 'EXPERIMENT_STATE_RUNNING', type: 'EXPERIMENT_TYPE_INTERLEAVING', variants: [], guardrailConfigs: [], secondaryMetricIds: [] },
            { experimentId: '4', name: 'd', state: 'EXPERIMENT_STATE_CONCLUDING', type: 'EXPERIMENT_TYPE_MAB', variants: [], guardrailConfigs: [], secondaryMetricIds: [] },
            { experimentId: '5', name: 'e', state: 'EXPERIMENT_STATE_CONCLUDED', type: 'EXPERIMENT_TYPE_CONTEXTUAL_BANDIT', variants: [], guardrailConfigs: [], secondaryMetricIds: [] },
            { experimentId: '6', name: 'f', state: 'EXPERIMENT_STATE_ARCHIVED', type: 'EXPERIMENT_TYPE_PLAYBACK_QOE', variants: [], guardrailConfigs: [], secondaryMetricIds: [] },
          ],
          nextPageToken: '',
        }),
      ),
    );

    const res = await listExperiments();
    expect(res.experiments.map((e) => e.state)).toEqual([
      'DRAFT', 'STARTING', 'RUNNING', 'CONCLUDING', 'CONCLUDED', 'ARCHIVED',
    ]);
    expect(res.experiments.map((e) => e.type)).toEqual([
      'AB', 'MVT', 'INTERLEAVING', 'MAB', 'CONTEXTUAL_BANDIT', 'PLAYBACK_QOE',
    ]);
  });
});

describe('API contract — ConnectRPC error responses', () => {
  it('parses ConnectRPC error with message field', async () => {
    server.use(
      http.post(`${MGMT_SVC}/GetExperiment`, () =>
        HttpResponse.json(
          { code: 'not_found', message: 'Experiment abc not found' },
          { status: 404 },
        ),
      ),
    );

    await expect(getExperiment('abc')).rejects.toThrow('Experiment abc not found');
  });

  it('parses ConnectRPC error with error field', async () => {
    server.use(
      http.post(`${ANALYSIS_SVC}/GetAnalysisResult`, () =>
        HttpResponse.json(
          { error: 'No analysis result for experiment xyz' },
          { status: 404 },
        ),
      ),
    );

    await expect(getAnalysisResult('xyz')).rejects.toThrow('No analysis result for experiment xyz');
  });

  it('falls back to status code when body is not JSON', async () => {
    server.use(
      http.post(`${MGMT_SVC}/GetExperiment`, () =>
        new HttpResponse('Internal Server Error', { status: 500 }),
      ),
    );

    await expect(getExperiment('fail')).rejects.toThrow('RPC GetExperiment failed: 500');
  });

  it('falls back to status code when body has no message', async () => {
    server.use(
      http.post(`${MGMT_SVC}/GetExperiment`, () =>
        HttpResponse.json({ code: 'internal' }, { status: 500 }),
      ),
    );

    await expect(getExperiment('fail')).rejects.toThrow('RPC GetExperiment failed: 500');
  });
});

describe('API contract — response envelope unwrapping', () => {
  it('unwraps experiment from { experiment: {...} } envelope', async () => {
    server.use(
      http.post(`${MGMT_SVC}/GetExperiment`, () =>
        HttpResponse.json({
          experiment: {
            experimentId: 'wrapped-1',
            name: 'Wrapped Test',
            state: 'DRAFT',
            type: 'AB',
            variants: [{ variantId: 'v1', name: 'control', trafficFraction: 0.5, isControl: true, payload: {} }],
            guardrailConfigs: [],
            secondaryMetricIds: [],
            createdAt: '2024-01-15T10:00:00Z',
          },
        }),
      ),
    );

    const exp = await getExperiment('wrapped-1');
    expect(exp.experimentId).toBe('wrapped-1');
    expect(exp.name).toBe('Wrapped Test');
    expect(exp.variants).toHaveLength(1);
    expect(exp.createdAt).toBe('2024-01-15T10:00:00Z');
  });

  it('unwraps experiments list from { experiments: [...] } envelope', async () => {
    server.use(
      http.post(`${MGMT_SVC}/ListExperiments`, () =>
        HttpResponse.json({
          experiments: [
            { experimentId: '1', name: 'first', state: 'DRAFT', type: 'AB', variants: [], guardrailConfigs: [], secondaryMetricIds: [] },
          ],
          nextPageToken: 'token-abc',
        }),
      ),
    );

    const res = await listExperiments();
    expect(res.experiments).toHaveLength(1);
    expect(res.nextPageToken).toBe('token-abc');
  });

  it('unwraps query log from { entries: [...] } envelope', async () => {
    const entries = await getQueryLog('11111111-1111-1111-1111-111111111111');
    expect(entries.length).toBeGreaterThan(0);
    expect(entries[0]).toHaveProperty('sqlText');
  });
});

describe('API contract — default/fallback values', () => {
  it('defaults missing optional fields', async () => {
    server.use(
      http.post(`${MGMT_SVC}/GetExperiment`, () =>
        HttpResponse.json({
          experiment: {
            experimentId: 'minimal-1',
            name: 'Minimal',
            variants: [],
            guardrailConfigs: [],
            secondaryMetricIds: [],
            // no state, type, description, ownerEmail, etc.
          },
        }),
      ),
    );

    const exp = await getExperiment('minimal-1');
    expect(exp.state).toBe('DRAFT');
    expect(exp.type).toBe('AB');
    expect(exp.description).toBe('');
    expect(exp.ownerEmail).toBe('');
    expect(exp.layerId).toBe('');
    expect(exp.hashSalt).toBe('');
    expect(exp.primaryMetricId).toBe('');
    expect(exp.isCumulativeHoldout).toBe(false);
    expect(exp.startedAt).toBeUndefined();
    expect(exp.concludedAt).toBeUndefined();
  });

  it('handles empty experiments list', async () => {
    server.use(
      http.post(`${MGMT_SVC}/ListExperiments`, () =>
        HttpResponse.json({}),
      ),
    );

    const res = await listExperiments();
    expect(res.experiments).toEqual([]);
    expect(res.nextPageToken).toBe('');
  });

  it('handles missing entries in query log', async () => {
    server.use(
      http.post(`${METRICS_SVC}/GetQueryLog`, () =>
        HttpResponse.json({}),
      ),
    );

    const entries = await getQueryLog('nonexistent');
    expect(entries).toEqual([]);
  });
});

describe('API contract — state transitions', () => {
  it('StartExperiment returns experiment with RUNNING state', async () => {
    const exp = await startExperiment('22222222-2222-2222-2222-222222222222');
    expect(exp.state).toBe('RUNNING');
    expect(exp.startedAt).toBeTruthy();
  });

  it('ConcludeExperiment returns experiment with CONCLUDED state', async () => {
    const exp = await concludeExperiment('11111111-1111-1111-1111-111111111111');
    expect(exp.state).toBe('CONCLUDED');
    expect(exp.concludedAt).toBeTruthy();
  });

  it('ArchiveExperiment returns error for non-CONCLUDED experiment', async () => {
    await expect(archiveExperiment('11111111-1111-1111-1111-111111111111'))
      .rejects.toThrow();
  });

  it('CreateExperiment returns new DRAFT experiment', async () => {
    const exp = await createExperiment({
      name: 'contract_test',
      description: 'Integration test experiment',
      type: 'AB',
      ownerEmail: 'test@example.com',
      variants: [],
      layerId: 'layer-test',
      primaryMetricId: 'test_metric',
      secondaryMetricIds: [],
      guardrailConfigs: [],
      guardrailAction: 'AUTO_PAUSE',
      isCumulativeHoldout: false,
    });
    expect(exp.state).toBe('DRAFT');
    expect(exp.name).toBe('contract_test');
    expect(exp.experimentId).toBeTruthy();
  });
});

describe('API contract — analysis services', () => {
  it('getAnalysisResult returns metric results', async () => {
    const result = await getAnalysisResult('11111111-1111-1111-1111-111111111111');
    expect(result.experimentId).toBe('11111111-1111-1111-1111-111111111111');
    expect(result.metricResults.length).toBeGreaterThan(0);
  });

  it('getNoveltyAnalysis returns novelty data', async () => {
    const result = await getNoveltyAnalysis('11111111-1111-1111-1111-111111111111');
    expect(result).toHaveProperty('noveltyDetected');
    expect(result).toHaveProperty('rawTreatmentEffect');
    expect(result).toHaveProperty('projectedSteadyStateEffect');
  });

  it('getInterferenceAnalysis returns interference data', async () => {
    const result = await getInterferenceAnalysis('11111111-1111-1111-1111-111111111111');
    expect(result).toHaveProperty('interferenceDetected');
    expect(result).toHaveProperty('jensenShannonDivergence');
  });

  it('getInterleavingAnalysis returns interleaving data', async () => {
    const result = await getInterleavingAnalysis('33333333-3333-3333-3333-333333333333');
    expect(result).toHaveProperty('algorithmWinRates');
    expect(result).toHaveProperty('signTestPValue');
    expect(result).toHaveProperty('algorithmStrengths');
  });

  it('getBanditDashboard returns bandit data', async () => {
    const result = await getBanditDashboard('44444444-4444-4444-4444-444444444444');
    expect(result.algorithm).toBe('THOMPSON_SAMPLING');
    expect(result.arms.length).toBeGreaterThan(0);
    expect(result.arms[0]).toHaveProperty('selectionCount');
    expect(result.arms[0]).toHaveProperty('rewardRate');
    expect(result.arms[0]).toHaveProperty('assignmentProbability');
  });

  it('exportNotebook returns base64 content and filename', async () => {
    const result = await exportNotebook('11111111-1111-1111-1111-111111111111');
    expect(result.content).toBeTruthy();
    expect(result.filename).toContain('.ipynb');
    // Verify the content is valid base64
    const decoded = atob(result.content);
    const parsed = JSON.parse(decoded);
    expect(parsed).toHaveProperty('metadata');
  });

  it('returns error for non-existent analysis', async () => {
    await expect(getNoveltyAnalysis('99999999-9999-9999-9999-999999999999'))
      .rejects.toThrow();
  });
});
