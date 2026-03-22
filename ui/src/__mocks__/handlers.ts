import { http, HttpResponse } from 'msw';
import {
  SEED_EXPERIMENTS, SEED_QUERY_LOG, SEED_ANALYSIS_RESULTS,
  SEED_NOVELTY_RESULTS, SEED_INTERFERENCE_RESULTS, SEED_INTERLEAVING_RESULTS,
  SEED_BANDIT_RESULTS, SEED_HOLDOUT_RESULTS, SEED_GUARDRAIL_STATUS, SEED_QOE_RESULTS,
  SEED_GST_RESULTS, SEED_CATE_RESULTS, SEED_LAYERS, SEED_LAYER_ALLOCATIONS,
  SEED_METRIC_DEFINITIONS, SEED_FLAGS,
} from './seed-data';
import type { UserRole } from '@/lib/auth';
import { hasAtLeast, isValidRole } from '@/lib/auth';

const MGMT_SVC = '*/experimentation.management.v1.ExperimentManagementService';
const METRICS_SVC = '*/experimentation.metrics.v1.MetricComputationService';
const ANALYSIS_SVC = '*/experimentation.analysis.v1.AnalysisService';
const BANDIT_SVC = '*/experimentation.bandit.v1.BanditPolicyService';
const FLAGS_SVC = '*/experimentation.flags.v1.FeatureFlagService';

// --- Mock auth enforcement ---
let _mockAuthEnabled = false;

export function enableMockAuth(): void {
  _mockAuthEnabled = true;
}

export function disableMockAuth(): void {
  _mockAuthEnabled = false;
}

function checkAuth(headers: Headers, requiredRole: UserRole) {
  if (!_mockAuthEnabled) return null;
  const roleHeader = headers.get('X-User-Role') || '';
  const role = isValidRole(roleHeader) ? roleHeader : 'viewer';
  if (!hasAtLeast(role, requiredRole)) {
    return HttpResponse.json(
      { code: 'permission_denied', message: `Requires ${requiredRole} role` },
      { status: 403 },
    );
  }
  return null;
}

export const handlers = [
  // ListExperiments (with optional server-side filters)
  http.post(`${MGMT_SVC}/ListExperiments`, async ({ request }) => {
    const body = await request.json() as Record<string, unknown>;
    let filtered = [...SEED_EXPERIMENTS];

    if (body.stateFilter) {
      const stateVal = (body.stateFilter as string).replace('EXPERIMENT_STATE_', '');
      filtered = filtered.filter((e) => e.state === stateVal);
    }
    if (body.typeFilter) {
      const typeVal = (body.typeFilter as string).replace('EXPERIMENT_TYPE_', '');
      filtered = filtered.filter((e) => e.type === typeVal);
    }
    if (body.ownerEmailFilter) {
      filtered = filtered.filter((e) => e.ownerEmail === body.ownerEmailFilter);
    }

    const pageSize = (body.pageSize as number) || filtered.length;
    const pageToken = (body.pageToken as string) || '';
    const startIndex = pageToken ? parseInt(pageToken, 10) : 0;
    const page = filtered.slice(startIndex, startIndex + pageSize);
    const nextIndex = startIndex + pageSize;
    const nextPageToken = nextIndex < filtered.length ? String(nextIndex) : '';

    return HttpResponse.json({
      experiments: page,
      nextPageToken,
    });
  }),

  // GetExperiment
  http.post(`${MGMT_SVC}/GetExperiment`, async ({ request }) => {
    const body = await request.json() as { experimentId: string };
    const experiment = SEED_EXPERIMENTS.find((e) => e.experimentId === body.experimentId);

    if (!experiment) {
      return HttpResponse.json(
        { code: 'not_found', message: `Experiment ${body.experimentId} not found` },
        { status: 404 },
      );
    }

    return HttpResponse.json({ experiment });
  }),

  // CreateExperiment
  http.post(`${MGMT_SVC}/CreateExperiment`, async ({ request }) => {
    const denied = checkAuth(request.headers,'experimenter');
    if (denied) return denied;
    const body = await request.json() as Record<string, unknown>;
    const newExperiment = {
      experimentId: crypto.randomUUID(),
      name: body.name as string,
      description: (body.description as string) || '',
      ownerEmail: (body.ownerEmail as string) || '',
      type: body.type as string,
      state: 'DRAFT' as const,
      variants: body.variants || [],
      layerId: (body.layerId as string) || '',
      hashSalt: `salt-${(body.name as string || '').replace(/\s+/g, '-').toLowerCase()}`,
      primaryMetricId: (body.primaryMetricId as string) || '',
      secondaryMetricIds: (body.secondaryMetricIds as string[]) || [],
      guardrailConfigs: body.guardrailConfigs || [],
      guardrailAction: (body.guardrailAction as string) || 'AUTO_PAUSE',
      sequentialTestConfig: body.sequentialTestConfig,
      targetingRuleId: body.targetingRuleId as string | undefined,
      isCumulativeHoldout: (body.isCumulativeHoldout as boolean) || false,
      interleavingConfig: body.interleavingConfig,
      sessionConfig: body.sessionConfig,
      banditExperimentConfig: body.banditExperimentConfig,
      qoeConfig: body.qoeConfig,
      createdAt: new Date().toISOString(),
    };

    SEED_EXPERIMENTS.push(newExperiment as typeof SEED_EXPERIMENTS[number]);
    return HttpResponse.json({ experiment: newExperiment });
  }),

  // UpdateExperiment
  http.post(`${MGMT_SVC}/UpdateExperiment`, async ({ request }) => {
    const denied = checkAuth(request.headers,'experimenter');
    if (denied) return denied;
    const body = await request.json() as { experiment: Record<string, unknown> };
    const exp = body.experiment;
    const id = exp.experimentId as string;
    const idx = SEED_EXPERIMENTS.findIndex((e) => e.experimentId === id);

    if (idx === -1) {
      return HttpResponse.json(
        { code: 'not_found', message: `Experiment ${id} not found` },
        { status: 404 },
      );
    }

    if (SEED_EXPERIMENTS[idx].state !== 'DRAFT') {
      return HttpResponse.json(
        { code: 'failed_precondition', message: 'Only DRAFT experiments can be updated' },
        { status: 400 },
      );
    }

    SEED_EXPERIMENTS[idx] = { ...SEED_EXPERIMENTS[idx], ...exp } as typeof SEED_EXPERIMENTS[number];
    return HttpResponse.json({ experiment: SEED_EXPERIMENTS[idx] });
  }),

  // StartExperiment: DRAFT → RUNNING (mock skips STARTING)
  http.post(`${MGMT_SVC}/StartExperiment`, async ({ request }) => {
    const denied = checkAuth(request.headers,'experimenter');
    if (denied) return denied;
    const body = await request.json() as { experimentId: string };
    const idx = SEED_EXPERIMENTS.findIndex((e) => e.experimentId === body.experimentId);

    if (idx === -1) {
      return HttpResponse.json(
        { code: 'not_found', message: `Experiment ${body.experimentId} not found` },
        { status: 404 },
      );
    }

    if (SEED_EXPERIMENTS[idx].state !== 'DRAFT') {
      return HttpResponse.json(
        { code: 'failed_precondition', message: 'Only DRAFT experiments can be started' },
        { status: 400 },
      );
    }

    SEED_EXPERIMENTS[idx] = {
      ...SEED_EXPERIMENTS[idx],
      state: 'RUNNING',
      startedAt: new Date().toISOString(),
    };
    return HttpResponse.json({ experiment: SEED_EXPERIMENTS[idx] });
  }),

  // ConcludeExperiment: RUNNING → CONCLUDED (mock skips CONCLUDING)
  http.post(`${MGMT_SVC}/ConcludeExperiment`, async ({ request }) => {
    const denied = checkAuth(request.headers,'experimenter');
    if (denied) return denied;
    const body = await request.json() as { experimentId: string };
    const idx = SEED_EXPERIMENTS.findIndex((e) => e.experimentId === body.experimentId);

    if (idx === -1) {
      return HttpResponse.json(
        { code: 'not_found', message: `Experiment ${body.experimentId} not found` },
        { status: 404 },
      );
    }

    if (SEED_EXPERIMENTS[idx].state !== 'RUNNING') {
      return HttpResponse.json(
        { code: 'failed_precondition', message: 'Only RUNNING experiments can be concluded' },
        { status: 400 },
      );
    }

    SEED_EXPERIMENTS[idx] = {
      ...SEED_EXPERIMENTS[idx],
      state: 'CONCLUDED',
      concludedAt: new Date().toISOString(),
    };
    return HttpResponse.json({ experiment: SEED_EXPERIMENTS[idx] });
  }),

  // ArchiveExperiment: CONCLUDED → ARCHIVED
  http.post(`${MGMT_SVC}/ArchiveExperiment`, async ({ request }) => {
    const denied = checkAuth(request.headers,'admin');
    if (denied) return denied;
    const body = await request.json() as { experimentId: string };
    const idx = SEED_EXPERIMENTS.findIndex((e) => e.experimentId === body.experimentId);

    if (idx === -1) {
      return HttpResponse.json(
        { code: 'not_found', message: `Experiment ${body.experimentId} not found` },
        { status: 404 },
      );
    }

    if (SEED_EXPERIMENTS[idx].state !== 'CONCLUDED') {
      return HttpResponse.json(
        { code: 'failed_precondition', message: 'Only CONCLUDED experiments can be archived' },
        { status: 400 },
      );
    }

    SEED_EXPERIMENTS[idx] = {
      ...SEED_EXPERIMENTS[idx],
      state: 'ARCHIVED',
    };
    return HttpResponse.json({ experiment: SEED_EXPERIMENTS[idx] });
  }),

  // PauseExperiment: RUNNING → PAUSED (mock adds PAUSED state)
  http.post(`${MGMT_SVC}/PauseExperiment`, async ({ request }) => {
    const denied = checkAuth(request.headers,'experimenter');
    if (denied) return denied;
    const body = await request.json() as { experimentId: string };
    const idx = SEED_EXPERIMENTS.findIndex((e) => e.experimentId === body.experimentId);

    if (idx === -1) {
      return HttpResponse.json(
        { code: 'not_found', message: `Experiment ${body.experimentId} not found` },
        { status: 404 },
      );
    }

    if (SEED_EXPERIMENTS[idx].state !== 'RUNNING') {
      return HttpResponse.json(
        { code: 'failed_precondition', message: 'Only RUNNING experiments can be paused' },
        { status: 400 },
      );
    }

    SEED_EXPERIMENTS[idx] = {
      ...SEED_EXPERIMENTS[idx],
      state: 'PAUSED' as typeof SEED_EXPERIMENTS[number]['state'],
    };
    return HttpResponse.json({ experiment: SEED_EXPERIMENTS[idx] });
  }),

  // ResumeExperiment: PAUSED → RUNNING
  http.post(`${MGMT_SVC}/ResumeExperiment`, async ({ request }) => {
    const denied = checkAuth(request.headers,'experimenter');
    if (denied) return denied;
    const body = await request.json() as { experimentId: string };
    const idx = SEED_EXPERIMENTS.findIndex((e) => e.experimentId === body.experimentId);

    if (idx === -1) {
      return HttpResponse.json(
        { code: 'not_found', message: `Experiment ${body.experimentId} not found` },
        { status: 404 },
      );
    }

    if (SEED_EXPERIMENTS[idx].state !== ('PAUSED' as string)) {
      return HttpResponse.json(
        { code: 'failed_precondition', message: 'Only PAUSED experiments can be resumed' },
        { status: 400 },
      );
    }

    SEED_EXPERIMENTS[idx] = {
      ...SEED_EXPERIMENTS[idx],
      state: 'RUNNING',
    };
    return HttpResponse.json({ experiment: SEED_EXPERIMENTS[idx] });
  }),

  // GetAnalysisResult
  http.post(`${ANALYSIS_SVC}/GetAnalysisResult`, async ({ request }) => {
    const body = await request.json() as { experimentId?: string };
    const experimentId = body.experimentId;

    if (!experimentId) {
      return HttpResponse.json(
        { error: 'experimentId is required' },
        { status: 400 },
      );
    }

    const result = SEED_ANALYSIS_RESULTS.find((r) => r.experimentId === experimentId);

    if (!result) {
      return HttpResponse.json(
        { error: `No analysis result for experiment ${experimentId}` },
        { status: 404 },
      );
    }

    return HttpResponse.json(result);
  }),

  // GetQueryLog
  http.post(`${METRICS_SVC}/GetQueryLog`, async ({ request }) => {
    const body = await request.json() as { experimentId: string; metricId?: string };
    let entries = SEED_QUERY_LOG[body.experimentId] || [];

    if (body.metricId) {
      entries = entries.filter((e) => e.metricId === body.metricId);
    }

    return HttpResponse.json({ entries });
  }),

  // ExportNotebook — realistic ~200KB payload with 25 cells
  http.post(`${METRICS_SVC}/ExportNotebook`, async ({ request }) => {
    const body = await request.json() as { experimentId: string };
    const experiment = SEED_EXPERIMENTS.find((e) => e.experimentId === body.experimentId);
    const name = experiment?.name || 'experiment';

    // Generate 25 cells with realistic SQL + output rows (~200KB total)
    const cells = Array.from({ length: 25 }, (_, i) => {
      const outputRows = Array.from({ length: 50 }, (_, r) =>
        `variant_${r % 3}\t${(Math.random() * 1000).toFixed(2)}\t${(Math.random() * 100).toFixed(4)}\t${(Math.random() * 50).toFixed(4)}\t${(Math.random()).toFixed(6)}\t${r + 1}\t${Date.now() - r * 86400000}`,
      ).join('\n');

      return {
        cell_type: 'code',
        source: [
          `# Metric ${i + 1}: ${['completion_rate', 'watch_time', 'ctr', 'rebuffer_ratio', 'search_ndcg'][i % 5]}`,
          `SELECT variant_id, COUNT(*) as n, AVG(value) as mean,`,
          `       STDDEV(value) as std, MIN(value) as min_val, MAX(value) as max_val,`,
          `       PERCENTILE_CONT(0.5) WITHIN GROUP (ORDER BY value) as median`,
          `FROM experiment_metrics`,
          `WHERE experiment_id = '${body.experimentId}'`,
          `  AND metric_id = 'metric_${i + 1}'`,
          `  AND event_timestamp >= '2026-01-01'`,
          `  AND event_timestamp < '2026-03-10'`,
          `GROUP BY variant_id`,
          `ORDER BY variant_id;`,
        ].join('\n'),
        outputs: [{
          output_type: 'execute_result',
          data: { 'text/plain': outputRows },
          metadata: {},
          execution_count: i + 1,
        }],
        metadata: { tags: [] },
        execution_count: i + 1,
      };
    });

    const notebook = {
      nbformat: 4,
      nbformat_minor: 5,
      metadata: {
        experiment_id: body.experimentId,
        experiment_name: name,
        generated_at: new Date().toISOString(),
        kernelspec: { display_name: 'Python 3', language: 'python', name: 'python3' },
      },
      cells,
    };

    return HttpResponse.json({
      content: btoa(JSON.stringify(notebook)),
      filename: `${name}_analysis.ipynb`,
    });
  }),

  // GetNoveltyAnalysis
  http.post(`${ANALYSIS_SVC}/GetNoveltyAnalysis`, async ({ request }) => {
    const body = await request.json() as { experimentId: string };
    const result = SEED_NOVELTY_RESULTS[body.experimentId];
    if (!result) {
      return HttpResponse.json(
        { error: `No novelty analysis for experiment ${body.experimentId}` },
        { status: 404 },
      );
    }
    return HttpResponse.json(result);
  }),

  // GetInterferenceAnalysis
  http.post(`${ANALYSIS_SVC}/GetInterferenceAnalysis`, async ({ request }) => {
    const body = await request.json() as { experimentId: string };
    const result = SEED_INTERFERENCE_RESULTS[body.experimentId];
    if (!result) {
      return HttpResponse.json(
        { error: `No interference analysis for experiment ${body.experimentId}` },
        { status: 404 },
      );
    }
    return HttpResponse.json(result);
  }),

  // GetInterleavingAnalysis
  http.post(`${ANALYSIS_SVC}/GetInterleavingAnalysis`, async ({ request }) => {
    const body = await request.json() as { experimentId: string };
    const result = SEED_INTERLEAVING_RESULTS[body.experimentId];
    if (!result) {
      return HttpResponse.json(
        { error: `No interleaving analysis for experiment ${body.experimentId}` },
        { status: 404 },
      );
    }
    return HttpResponse.json(result);
  }),

  // GetBanditDashboard
  http.post(`${BANDIT_SVC}/GetBanditDashboard`, async ({ request }) => {
    const body = await request.json() as { experimentId: string };
    const result = SEED_BANDIT_RESULTS[body.experimentId];
    if (!result) {
      return HttpResponse.json(
        { error: `No bandit dashboard for experiment ${body.experimentId}` },
        { status: 404 },
      );
    }
    return HttpResponse.json(result);
  }),

  // GetCumulativeHoldoutResult
  http.post(`${ANALYSIS_SVC}/GetCumulativeHoldoutResult`, async ({ request }) => {
    const body = await request.json() as { experimentId: string };
    const result = SEED_HOLDOUT_RESULTS[body.experimentId];
    if (!result) {
      return HttpResponse.json(
        { error: `No holdout result for experiment ${body.experimentId}` },
        { status: 404 },
      );
    }
    return HttpResponse.json(result);
  }),

  // GetGstTrajectory
  http.post(`${ANALYSIS_SVC}/GetGstTrajectory`, async ({ request }) => {
    const body = await request.json() as { experimentId: string; metricId: string };
    const results = SEED_GST_RESULTS[body.experimentId];
    if (!results) {
      return HttpResponse.json(
        { error: `No GST trajectory for experiment ${body.experimentId}` },
        { status: 404 },
      );
    }
    const result = results.find((r) => r.metricId === body.metricId);
    if (!result) {
      return HttpResponse.json(
        { error: `No GST trajectory for metric ${body.metricId}` },
        { status: 404 },
      );
    }
    return HttpResponse.json(result);
  }),

  // GetQoeDashboard
  http.post(`${ANALYSIS_SVC}/GetQoeDashboard`, async ({ request }) => {
    const body = await request.json() as { experimentId: string };
    const result = SEED_QOE_RESULTS[body.experimentId];
    if (!result) {
      return HttpResponse.json(
        { error: `No QoE dashboard for experiment ${body.experimentId}` },
        { status: 404 },
      );
    }
    return HttpResponse.json(result);
  }),

  // GetGuardrailStatus
  http.post(`${MGMT_SVC}/GetGuardrailStatus`, async ({ request }) => {
    const body = await request.json() as { experimentId: string };
    const result = SEED_GUARDRAIL_STATUS[body.experimentId];
    if (!result) {
      return HttpResponse.json({
        experimentId: body.experimentId,
        breaches: [],
        isPaused: false,
      });
    }
    return HttpResponse.json(result);
  }),

  // GetCateAnalysis
  http.post(`${ANALYSIS_SVC}/GetCateAnalysis`, async ({ request }) => {
    const body = await request.json() as { experimentId: string };
    const result = SEED_CATE_RESULTS[body.experimentId];
    if (!result) {
      return HttpResponse.json(
        { error: `No CATE analysis for experiment ${body.experimentId}` },
        { status: 404 },
      );
    }
    return HttpResponse.json(result);
  }),

  // GetLayer
  http.post(`${MGMT_SVC}/GetLayer`, async ({ request }) => {
    const body = await request.json() as { layerId: string };
    const layer = SEED_LAYERS[body.layerId];
    if (!layer) {
      return HttpResponse.json(
        { code: 'not_found', message: `Layer ${body.layerId} not found` },
        { status: 404 },
      );
    }
    return HttpResponse.json(layer);
  }),

  // GetLayerAllocations
  http.post(`${MGMT_SVC}/GetLayerAllocations`, async ({ request }) => {
    const body = await request.json() as { layerId: string; includeReleased?: boolean };
    let allocations = SEED_LAYER_ALLOCATIONS[body.layerId] || [];
    if (!body.includeReleased) {
      allocations = allocations.filter((a) => !a.releasedAt);
    }
    return HttpResponse.json({ allocations });
  }),

  // ListMetricDefinitions — mirrors Agent-5 proto wire format:
  //   - Enum values use METRIC_TYPE_ prefix (adapter strips on client)
  //   - Proto3 zero-value omission: false booleans and 0 numbers are absent from JSON
  //   - typeFilter field pending Agent-5 proto addition (additive, field 3)
  http.post(`${MGMT_SVC}/ListMetricDefinitions`, async ({ request }) => {
    const body = await request.json() as Record<string, unknown>;

    // Convert seed data to proto wire format
    let metrics = SEED_METRIC_DEFINITIONS.map((m) => {
      const wire: Record<string, unknown> = {
        metricId: m.metricId,
        name: m.name,
        description: m.description,
        type: `METRIC_TYPE_${m.type}`,
        sourceEventType: m.sourceEventType,
      };
      // Proto3: only include non-default values
      if (m.lowerIsBetter) wire.lowerIsBetter = true;
      if (m.isQoeMetric) wire.isQoeMetric = true;
      if (m.numeratorEventType) wire.numeratorEventType = m.numeratorEventType;
      if (m.denominatorEventType) wire.denominatorEventType = m.denominatorEventType;
      if (m.percentile) wire.percentile = m.percentile;
      if (m.customSql) wire.customSql = m.customSql;
      if (m.surrogateTargetMetricId) wire.surrogateTargetMetricId = m.surrogateTargetMetricId;
      if (m.cupedCovariateMetricId) wire.cupedCovariateMetricId = m.cupedCovariateMetricId;
      if (m.minimumDetectableEffect) wire.minimumDetectableEffect = m.minimumDetectableEffect;
      return wire;
    });

    // Server-side type filter (pending Agent-5 proto addition)
    if (body.typeFilter) {
      const typeVal = body.typeFilter as string;
      metrics = metrics.filter((m) => m.type === typeVal);
    }

    const pageSize = (body.pageSize as number) || metrics.length;
    const pageToken = (body.pageToken as string) || '';
    const startIndex = pageToken ? parseInt(pageToken, 10) : 0;
    const page = metrics.slice(startIndex, startIndex + pageSize);
    const nextIndex = startIndex + pageSize;
    const nextPageToken = nextIndex < metrics.length ? String(nextIndex) : '';

    return HttpResponse.json({ metrics: page, nextPageToken });
  }),

  // --- Feature Flags (M7) ---

  // ListFlags — mirrors M7 proto wire format with FLAG_TYPE_ prefix
  http.post(`${FLAGS_SVC}/ListFlags`, async ({ request }) => {
    const body = await request.json() as Record<string, unknown>;

    const flags = SEED_FLAGS.map((f) => {
      const wire: Record<string, unknown> = {
        flagId: f.flagId,
        name: f.name,
        type: `FLAG_TYPE_${f.type}`,
        defaultValue: f.defaultValue,
      };
      if (f.description) wire.description = f.description;
      if (f.enabled) wire.enabled = true;
      if (f.rolloutPercentage) wire.rolloutPercentage = f.rolloutPercentage;
      if (f.variants.length > 0) wire.variants = f.variants;
      if (f.targetingRuleId) wire.targetingRuleId = f.targetingRuleId;
      return wire;
    });

    const pageSize = (body.pageSize as number) || flags.length;
    const pageToken = (body.pageToken as string) || '';
    const startIndex = pageToken ? parseInt(pageToken, 10) : 0;
    const page = flags.slice(startIndex, startIndex + pageSize);
    const nextIndex = startIndex + pageSize;
    const nextPageToken = nextIndex < flags.length ? String(nextIndex) : '';

    return HttpResponse.json({ flags: page, nextPageToken });
  }),

  // GetFlag
  http.post(`${FLAGS_SVC}/GetFlag`, async ({ request }) => {
    const body = await request.json() as { flagId: string };
    const flag = SEED_FLAGS.find((f) => f.flagId === body.flagId);

    if (!flag) {
      return HttpResponse.json(
        { code: 'not_found', message: `Flag ${body.flagId} not found` },
        { status: 404 },
      );
    }

    const wire: Record<string, unknown> = {
      flagId: flag.flagId,
      name: flag.name,
      type: `FLAG_TYPE_${flag.type}`,
      defaultValue: flag.defaultValue,
    };
    if (flag.description) wire.description = flag.description;
    if (flag.enabled) wire.enabled = true;
    if (flag.rolloutPercentage) wire.rolloutPercentage = flag.rolloutPercentage;
    if (flag.variants.length > 0) wire.variants = flag.variants;
    if (flag.targetingRuleId) wire.targetingRuleId = flag.targetingRuleId;

    return HttpResponse.json(wire);
  }),

  // CreateFlag — proto: CreateFlagRequest { Flag flag = 1; }
  http.post(`${FLAGS_SVC}/CreateFlag`, async ({ request }) => {
    const denied = checkAuth(request.headers, 'experimenter');
    if (denied) return denied;
    const body = await request.json() as { flag?: Record<string, unknown> };
    const f = body.flag || {};

    const newFlag = {
      flagId: crypto.randomUUID(),
      name: (f.name as string) || '',
      description: (f.description as string) || '',
      type: (f.type as string) || 'BOOLEAN',
      defaultValue: (f.defaultValue as string) || '',
      enabled: (f.enabled as boolean) || false,
      rolloutPercentage: (f.rolloutPercentage as number) || 0,
      variants: (f.variants as typeof SEED_FLAGS[number]['variants']) || [],
      targetingRuleId: f.targetingRuleId as string | undefined,
    };

    SEED_FLAGS.push(newFlag as typeof SEED_FLAGS[number]);
    return HttpResponse.json({
      flagId: newFlag.flagId,
      name: newFlag.name,
      type: `FLAG_TYPE_${newFlag.type}`,
      defaultValue: newFlag.defaultValue,
      ...(newFlag.description ? { description: newFlag.description } : {}),
      ...(newFlag.enabled ? { enabled: true } : {}),
      ...(newFlag.rolloutPercentage ? { rolloutPercentage: newFlag.rolloutPercentage } : {}),
      ...(newFlag.variants.length > 0 ? { variants: newFlag.variants } : {}),
      ...(newFlag.targetingRuleId ? { targetingRuleId: newFlag.targetingRuleId } : {}),
    });
  }),

  // UpdateFlag
  http.post(`${FLAGS_SVC}/UpdateFlag`, async ({ request }) => {
    const denied = checkAuth(request.headers, 'experimenter');
    if (denied) return denied;
    const body = await request.json() as { flag: Record<string, unknown> };
    const flagData = body.flag;
    const id = flagData.flagId as string;
    const idx = SEED_FLAGS.findIndex((f) => f.flagId === id);

    if (idx === -1) {
      return HttpResponse.json(
        { code: 'not_found', message: `Flag ${id} not found` },
        { status: 404 },
      );
    }

    SEED_FLAGS[idx] = { ...SEED_FLAGS[idx], ...flagData } as typeof SEED_FLAGS[number];
    const updated = SEED_FLAGS[idx];
    const wire: Record<string, unknown> = {
      flagId: updated.flagId,
      name: updated.name,
      type: `FLAG_TYPE_${updated.type}`,
      defaultValue: updated.defaultValue,
    };
    if (updated.description) wire.description = updated.description;
    if (updated.enabled) wire.enabled = true;
    if (updated.rolloutPercentage) wire.rolloutPercentage = updated.rolloutPercentage;
    if (updated.variants.length > 0) wire.variants = updated.variants;
    if (updated.targetingRuleId) wire.targetingRuleId = updated.targetingRuleId;
    return HttpResponse.json(wire);
  }),

  // EvaluateFlag
  http.post(`${FLAGS_SVC}/EvaluateFlag`, async ({ request }) => {
    const body = await request.json() as { flagId: string; userId: string };
    return HttpResponse.json({
      flagId: body.flagId,
      value: 'true',
      variantId: 'v-default',
    });
  }),

  // PromoteToExperiment
  http.post(`${FLAGS_SVC}/PromoteToExperiment`, async ({ request }) => {
    const denied = checkAuth(request.headers, 'experimenter');
    if (denied) return denied;
    const body = await request.json() as { flagId: string; experimentType: string; primaryMetricId: string };
    const flag = SEED_FLAGS.find((f) => f.flagId === body.flagId);

    if (!flag) {
      return HttpResponse.json(
        { code: 'not_found', message: `Flag ${body.flagId} not found` },
        { status: 404 },
      );
    }

    return HttpResponse.json({
      experimentId: `exp-from-${flag.flagId}`,
      name: flag.name,
      type: body.experimentType || 'EXPERIMENT_TYPE_AB',
      state: 'EXPERIMENT_STATE_DRAFT',
      variants: [
        { variantId: 'v1', name: 'control', trafficFraction: 0.5, isControl: true },
        { variantId: 'v2', name: 'treatment', trafficFraction: 0.5 },
      ],
      primaryMetricId: body.primaryMetricId,
      createdAt: new Date().toISOString(),
    });
  }),
];
