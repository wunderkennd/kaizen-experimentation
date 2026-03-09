import { http, HttpResponse } from 'msw';
import {
  SEED_EXPERIMENTS, SEED_QUERY_LOG, SEED_ANALYSIS_RESULTS,
  SEED_NOVELTY_RESULTS, SEED_INTERFERENCE_RESULTS, SEED_INTERLEAVING_RESULTS,
  SEED_BANDIT_RESULTS, SEED_HOLDOUT_RESULTS, SEED_GUARDRAIL_STATUS, SEED_QOE_RESULTS,
  SEED_GST_RESULTS,
} from './seed-data';

const MGMT_SVC = '*/experimentation.management.v1.ExperimentManagementService';
const METRICS_SVC = '*/experimentation.metrics.v1.MetricComputationService';
const ANALYSIS_SVC = '*/experimentation.analysis.v1.AnalysisService';
const BANDIT_SVC = '*/experimentation.bandit.v1.BanditPolicyService';

export const handlers = [
  // ListExperiments
  http.post(`${MGMT_SVC}/ListExperiments`, async () => {
    return HttpResponse.json({
      experiments: SEED_EXPERIMENTS,
      nextPageToken: '',
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
      createdAt: new Date().toISOString(),
    };

    SEED_EXPERIMENTS.push(newExperiment as typeof SEED_EXPERIMENTS[number]);
    return HttpResponse.json({ experiment: newExperiment });
  }),

  // UpdateExperiment
  http.post(`${MGMT_SVC}/UpdateExperiment`, async ({ request }) => {
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

  // ExportNotebook
  http.post(`${METRICS_SVC}/ExportNotebook`, async ({ request }) => {
    const body = await request.json() as { experimentId: string };
    const experiment = SEED_EXPERIMENTS.find((e) => e.experimentId === body.experimentId);
    const name = experiment?.name || 'experiment';

    return HttpResponse.json({
      content: btoa(`{"cells": [], "metadata": {"experiment_id": "${body.experimentId}"}}`),
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
];
