import { http, HttpResponse } from 'msw';
import { SEED_EXPERIMENTS, SEED_QUERY_LOG } from './seed-data';

const MGMT_SVC = '*/experimentation.management.v1.ExperimentManagementService';
const METRICS_SVC = '*/experimentation.metrics.v1.MetricComputationService';

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
];
