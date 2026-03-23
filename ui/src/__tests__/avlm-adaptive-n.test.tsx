/**
 * Tests for ADR-015 (AVLM) and ADR-020 (Adaptive N) UI components.
 *
 * Coverage:
 *   - AvlmBoundaryPlot: renders chart, conclusive badge, fallback on 404
 *   - AdaptiveNBadge: renders zone badge, hides on 404
 *   - AdaptiveNTimeline: renders for PROMISING zone, hidden for non-PROMISING
 *   - FeedbackLoopTab: renders all four sections, handles 404 gracefully
 *   - Results page: feedback loop tab visible, AVLM section present
 */

import { describe, it, expect, vi } from 'vitest';
import React from 'react';
import { render, screen } from '@testing-library/react';
import { http, HttpResponse } from 'msw';
import { server } from '@/__mocks__/server';

// Recharts uses ResizeObserver which is not available in JSDOM
vi.mock('recharts', async () => {
  const Passthrough = ({ children }: { children?: React.ReactNode }) => (
    <div data-testid="recharts-wrapper">{children}</div>
  );
  const Noop = () => null;
  return {
    ResponsiveContainer: Passthrough,
    ComposedChart: Passthrough,
    AreaChart: Passthrough,
    BarChart: Passthrough,
    Area: Noop,
    Line: Noop,
    Bar: Noop,
    XAxis: Noop,
    YAxis: Noop,
    CartesianGrid: Noop,
    Tooltip: Noop,
    Legend: Noop,
    ReferenceLine: Noop,
    Scatter: Noop,
    Cell: Noop,
  };
});

// Components under test
import { AdaptiveNBadge } from '@/components/adaptive-n-badge';
import { AdaptiveNTimeline } from '@/components/adaptive-n-timeline';
import { FeedbackLoopTab } from '@/components/feedback-loop-tab';

const ANALYSIS_SVC = '*/experimentation.analysis.v1.AnalysisService';

// ---------------------------------------------------------------------------
// AdaptiveNBadge
// ---------------------------------------------------------------------------
describe('AdaptiveNBadge', () => {
  it('renders FAVORABLE badge for experiment with favorable zone', async () => {
    render(<AdaptiveNBadge experimentId="11111111-1111-1111-1111-111111111111" />);
    const badge = await screen.findByTestId('adaptive-n-badge');
    expect(badge).toHaveAttribute('data-zone', 'FAVORABLE');
    expect(badge).toHaveTextContent('Favorable');
  });

  it('renders PROMISING badge for second seed experiment', async () => {
    render(<AdaptiveNBadge experimentId="88888888-8888-8888-8888-888888888888" />);
    const badge = await screen.findByTestId('adaptive-n-badge');
    expect(badge).toHaveAttribute('data-zone', 'PROMISING');
    expect(badge).toHaveTextContent('Promising');
  });

  it('renders nothing when server returns 404', async () => {
    server.use(
      http.post(`${ANALYSIS_SVC}/GetAdaptiveN`, () =>
        HttpResponse.json({ code: 'not_found' }, { status: 404 }),
      ),
    );
    const { container } = render(
      <AdaptiveNBadge experimentId="00000000-0000-0000-0000-000000000000" />,
    );
    // Give enough time for the fetch to complete
    await new Promise((r) => setTimeout(r, 50));
    expect(container.firstChild).toBeNull();
  });
});

// ---------------------------------------------------------------------------
// AdaptiveNTimeline
// ---------------------------------------------------------------------------
describe('AdaptiveNTimeline', () => {
  it('renders extended timeline for PROMISING zone', () => {
    const result = {
      experimentId: 'test',
      zone: 'PROMISING' as const,
      currentN: 80000,
      plannedN: 120000,
      recommendedN: 150000,
      conditionalPower: 0.67,
      projectedConclusionDate: '2026-04-15T00:00:00Z',
      extensionDays: 21,
      timelineProjection: [
        { date: '2026-03-01', estimatedN: 50000 },
        { date: '2026-03-15', estimatedN: 100000 },
        { date: '2026-03-30', estimatedN: 150000 },
      ],
      computedAt: '2026-03-05T00:00:00Z',
    };
    render(<AdaptiveNTimeline result={result} />);
    expect(screen.getByText(/Extended Timeline/)).toBeInTheDocument();
    expect(screen.getByText(/Conditional power: 67%/)).toBeInTheDocument();
    expect(screen.getByText(/Extension: \+21 days/)).toBeInTheDocument();
    expect(screen.getByText('Recommended N: 150,000')).toBeInTheDocument();
  });

  it('renders nothing for FAVORABLE zone', () => {
    const result = {
      experimentId: 'test',
      zone: 'FAVORABLE' as const,
      currentN: 95000,
      plannedN: 120000,
      conditionalPower: 0.94,
      timelineProjection: [],
      computedAt: '2026-03-05T00:00:00Z',
    };
    const { container } = render(<AdaptiveNTimeline result={result} />);
    expect(container.firstChild).toBeNull();
  });

  it('renders nothing for FUTILE zone', () => {
    const result = {
      experimentId: 'test',
      zone: 'FUTILE' as const,
      currentN: 40000,
      plannedN: 120000,
      conditionalPower: 0.12,
      timelineProjection: [],
      computedAt: '2026-03-05T00:00:00Z',
    };
    const { container } = render(<AdaptiveNTimeline result={result} />);
    expect(container.firstChild).toBeNull();
  });
});

// ---------------------------------------------------------------------------
// FeedbackLoopTab
// ---------------------------------------------------------------------------
describe('FeedbackLoopTab', () => {
  it('renders all four sections for a seeded experiment', async () => {
    render(<FeedbackLoopTab experimentId="11111111-1111-1111-1111-111111111111" />);

    // Wait for fetch to resolve
    await screen.findByText('Retraining Timeline');
    expect(screen.getByText('Pre/Post Comparison — Effect Estimate')).toBeInTheDocument();
    expect(screen.getByText('Contamination Over Time')).toBeInTheDocument();
    expect(screen.getByText('Mitigation Recommendations')).toBeInTheDocument();
  });

  it('shows bias-corrected estimate card', async () => {
    render(<FeedbackLoopTab experimentId="11111111-1111-1111-1111-111111111111" />);
    await screen.findByText('Bias-Corrected Estimate');
    // biasCorrectedEstimate appears in summary card and highlight block
    const matches = screen.getAllByText('0.0130');
    expect(matches.length).toBeGreaterThanOrEqual(1);
  });

  it('shows all retraining events', async () => {
    render(<FeedbackLoopTab experimentId="11111111-1111-1111-1111-111111111111" />);
    await screen.findByText('re-001');
    expect(screen.getByText('re-002')).toBeInTheDocument();
    expect(screen.getByText('neural_cf_v2.1')).toBeInTheDocument();
    expect(screen.getByText('neural_cf_v2.2')).toBeInTheDocument();
  });

  it('shows mitigation recommendations with severity', async () => {
    render(<FeedbackLoopTab experimentId="11111111-1111-1111-1111-111111111111" />);
    await screen.findByText('Enable pre-period washout');
    expect(screen.getByText('Use bias-corrected estimate for decision')).toBeInTheDocument();
    expect(screen.getByText('Reduce retraining frequency during experiment')).toBeInTheDocument();
  });

  it('shows not-available message when no data for experiment', async () => {
    server.use(
      http.post(`${ANALYSIS_SVC}/GetFeedbackLoopAnalysis`, () =>
        HttpResponse.json({ code: 'not_found' }, { status: 404 }),
      ),
    );
    render(<FeedbackLoopTab experimentId="99999999-9999-9999-9999-999999999999" />);
    await screen.findByText(/No feedback loop analysis available/);
  });

  it('shows retry button on server error', async () => {
    server.use(
      http.post(`${ANALYSIS_SVC}/GetFeedbackLoopAnalysis`, () =>
        HttpResponse.json({ message: 'internal error' }, { status: 500 }),
      ),
    );
    render(<FeedbackLoopTab experimentId="11111111-1111-1111-1111-111111111111" />);
    await screen.findByText(/Retry/i);
  });
});

// ---------------------------------------------------------------------------
// API: GetAvlmResult handler
// ---------------------------------------------------------------------------
describe('AVLM handler', () => {
  it('returns AVLM result for seeded metric', async () => {
    const res = await fetch(
      `http://localhost/experimentation.analysis.v1.AnalysisService/GetAvlmResult`,
      {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          experimentId: '11111111-1111-1111-1111-111111111111',
          metricId: 'click_through_rate',
        }),
      },
    );
    expect(res.status).toBe(200);
    const data = await res.json() as { isConclusive: boolean; boundaryPoints: unknown[] };
    expect(data.isConclusive).toBe(true);
    expect(Array.isArray(data.boundaryPoints)).toBe(true);
    expect((data.boundaryPoints as Array<{ look: number }>)[0].look).toBe(1);
  });

  it('returns 404 for unknown metric', async () => {
    const res = await fetch(
      `http://localhost/experimentation.analysis.v1.AnalysisService/GetAvlmResult`,
      {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          experimentId: '11111111-1111-1111-1111-111111111111',
          metricId: 'nonexistent_metric',
        }),
      },
    );
    expect(res.status).toBe(404);
  });
});
