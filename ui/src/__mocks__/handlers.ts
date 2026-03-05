import { http, HttpResponse } from 'msw';
import { SEED_EXPERIMENTS } from './seed-data';

export const handlers = [
  http.get('*/api/experiments', ({ request }) => {
    const url = new URL(request.url);
    const stateFilter = url.searchParams.get('state');
    const typeFilter = url.searchParams.get('type');

    let experiments = [...SEED_EXPERIMENTS];

    if (stateFilter) {
      experiments = experiments.filter((e) => e.state === stateFilter);
    }
    if (typeFilter) {
      experiments = experiments.filter((e) => e.type === typeFilter);
    }

    return HttpResponse.json({
      experiments,
      totalCount: experiments.length,
    });
  }),

  http.get('*/api/experiments/:id', ({ params }) => {
    const { id } = params;
    const experiment = SEED_EXPERIMENTS.find((e) => e.experimentId === id);

    if (!experiment) {
      return HttpResponse.json(
        { error: `Experiment ${id} not found` },
        { status: 404 },
      );
    }

    return HttpResponse.json(experiment);
  }),

  // Update experiment (DRAFT only)
  http.put('*/api/experiments/:id', async ({ params, request }) => {
    const { id } = params;
    const idx = SEED_EXPERIMENTS.findIndex((e) => e.experimentId === id);

    if (idx === -1) {
      return HttpResponse.json(
        { error: `Experiment ${id} not found` },
        { status: 404 },
      );
    }

    if (SEED_EXPERIMENTS[idx].state !== 'DRAFT') {
      return HttpResponse.json(
        { error: 'Only DRAFT experiments can be updated' },
        { status: 400 },
      );
    }

    const body = await request.json() as Record<string, unknown>;
    SEED_EXPERIMENTS[idx] = { ...SEED_EXPERIMENTS[idx], ...body } as typeof SEED_EXPERIMENTS[number];
    return HttpResponse.json(SEED_EXPERIMENTS[idx]);
  }),

  // Start experiment: DRAFT → RUNNING (mock skips STARTING)
  http.post('*/api/experiments/:id/start', ({ params }) => {
    const { id } = params;
    const idx = SEED_EXPERIMENTS.findIndex((e) => e.experimentId === id);

    if (idx === -1) {
      return HttpResponse.json(
        { error: `Experiment ${id} not found` },
        { status: 404 },
      );
    }

    if (SEED_EXPERIMENTS[idx].state !== 'DRAFT') {
      return HttpResponse.json(
        { error: 'Only DRAFT experiments can be started' },
        { status: 400 },
      );
    }

    SEED_EXPERIMENTS[idx] = {
      ...SEED_EXPERIMENTS[idx],
      state: 'RUNNING',
      startedAt: new Date().toISOString(),
    };
    return HttpResponse.json(SEED_EXPERIMENTS[idx]);
  }),

  // Conclude experiment: RUNNING → CONCLUDED (mock skips CONCLUDING)
  http.post('*/api/experiments/:id/conclude', ({ params }) => {
    const { id } = params;
    const idx = SEED_EXPERIMENTS.findIndex((e) => e.experimentId === id);

    if (idx === -1) {
      return HttpResponse.json(
        { error: `Experiment ${id} not found` },
        { status: 404 },
      );
    }

    if (SEED_EXPERIMENTS[idx].state !== 'RUNNING') {
      return HttpResponse.json(
        { error: 'Only RUNNING experiments can be concluded' },
        { status: 400 },
      );
    }

    SEED_EXPERIMENTS[idx] = {
      ...SEED_EXPERIMENTS[idx],
      state: 'CONCLUDED',
      concludedAt: new Date().toISOString(),
    };
    return HttpResponse.json(SEED_EXPERIMENTS[idx]);
  }),

  // Archive experiment: CONCLUDED → ARCHIVED
  http.post('*/api/experiments/:id/archive', ({ params }) => {
    const { id } = params;
    const idx = SEED_EXPERIMENTS.findIndex((e) => e.experimentId === id);

    if (idx === -1) {
      return HttpResponse.json(
        { error: `Experiment ${id} not found` },
        { status: 404 },
      );
    }

    if (SEED_EXPERIMENTS[idx].state !== 'CONCLUDED') {
      return HttpResponse.json(
        { error: 'Only CONCLUDED experiments can be archived' },
        { status: 400 },
      );
    }

    SEED_EXPERIMENTS[idx] = {
      ...SEED_EXPERIMENTS[idx],
      state: 'ARCHIVED',
    };
    return HttpResponse.json(SEED_EXPERIMENTS[idx]);
  }),
];
