/**
 * Tests for ADR-021 feedback loop interference UI components.
 *
 * Coverage:
 *   - FeedbackLoopAlert: WARNING severity for bias ≤ 0.1, ERROR for bias > 0.1
 *   - FeedbackLoopAlert: shows contamination metric, estimated bias, last retrain date
 *   - FeedbackLoopAlert: hidden when no feedback loop data (404)
 *   - FeedbackLoopAlert: hidden when contamination fraction is 0
 *   - InterferenceTimelineChart: renders chart section with retrain markers copy
 *   - InterferenceTimelineChart: returns null when no data points
 */

import { describe, it, expect, vi } from 'vitest';
import React from 'react';
import { render, screen } from '@testing-library/react';
import { http, HttpResponse } from 'msw';
import { server } from '@/__mocks__/server';
import type { FeedbackLoopResult } from '@/lib/types';

vi.mock('recharts', async () => {
  const Passthrough = ({ children }: { children?: React.ReactNode }) => (
    <div data-testid="recharts-wrapper">{children}</div>
  );
  const Noop = () => null;
  return {
    ResponsiveContainer: Passthrough,
    LineChart: Passthrough,
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

import { FeedbackLoopAlert } from '@/components/feedback-loop-alert';
import { InterferenceTimelineChart } from '@/components/interference-timeline-chart';

const ANALYSIS_SVC = '*/experimentation.analysis.v1.AnalysisService';

// Shared base result (bias = 0.001 < 0.1 → WARNING)
const BASE_RESULT: FeedbackLoopResult = {
  experimentId: '11111111-1111-1111-1111-111111111111',
  retrainingEvents: [
    {
      eventId: 're-001',
      retrainedAt: '2026-02-22T02:00:00Z',
      triggerReason: 'Scheduled weekly retrain',
      modelVersion: 'neural_cf_v2.1',
    },
    {
      eventId: 're-002',
      retrainedAt: '2026-03-01T02:00:00Z',
      triggerReason: 'Scheduled weekly retrain',
      modelVersion: 'neural_cf_v2.2',
    },
  ],
  prePostComparison: [
    { date: '2026-02-20', preEffect: 0.011, postEffect: 0.014 },
    { date: '2026-02-22', preEffect: 0.013, postEffect: 0.014 },
    { date: '2026-03-01', preEffect: 0.014, postEffect: 0.016 },
    { date: '2026-03-03', preEffect: 0.014, postEffect: 0.014 },
  ],
  contaminationTimeline: [
    { date: '2026-02-22', contaminationFraction: 0.0 },
    { date: '2026-02-24', contaminationFraction: 0.15 },
  ],
  rawEstimate: 0.014,
  biasCorrectedEstimate: 0.013,
  biasCorrectedCiLower: 0.004,
  biasCorrectedCiUpper: 0.022,
  contaminationFraction: 0.20,
  recommendations: [],
  computedAt: '2026-03-05T14:30:00Z',
};

// ---------------------------------------------------------------------------
// FeedbackLoopAlert
// ---------------------------------------------------------------------------
describe('FeedbackLoopAlert', () => {
  it('renders WARNING banner for seeded experiment (bias = 0.001 < 0.1)', async () => {
    render(
      <FeedbackLoopAlert
        experimentId="11111111-1111-1111-1111-111111111111"
        primaryMetricId="click_through_rate"
      />,
    );
    const alert = await screen.findByTestId('feedback-loop-alert');
    expect(alert).toHaveAttribute('data-severity', 'WARNING');
    expect(alert).toHaveTextContent(/Feedback Loop Interference Detected/);
  });

  it('shows contamination metric name', async () => {
    render(
      <FeedbackLoopAlert
        experimentId="11111111-1111-1111-1111-111111111111"
        primaryMetricId="click_through_rate"
      />,
    );
    const metricEl = await screen.findByTestId('alert-metric-name');
    expect(metricEl).toHaveTextContent('click_through_rate');
  });

  it('shows contamination percentage', async () => {
    render(
      <FeedbackLoopAlert experimentId="11111111-1111-1111-1111-111111111111" />,
    );
    const el = await screen.findByTestId('alert-contamination');
    expect(el).toHaveTextContent('20.0%');
  });

  it('shows estimated bias value', async () => {
    render(
      <FeedbackLoopAlert experimentId="11111111-1111-1111-1111-111111111111" />,
    );
    const el = await screen.findByTestId('alert-bias');
    // |0.014 - 0.013| = 0.001
    expect(el).toHaveTextContent('0.0010');
  });

  it('shows last retrain date', async () => {
    render(
      <FeedbackLoopAlert experimentId="11111111-1111-1111-1111-111111111111" />,
    );
    const el = await screen.findByTestId('alert-retrain-time');
    // Most recent event: 2026-03-01
    expect(el.textContent).toMatch(/Mar 1, 2026/);
  });

  it('renders ERROR severity when bias > 0.1', async () => {
    server.use(
      http.post(`${ANALYSIS_SVC}/GetFeedbackLoopAnalysis`, () =>
        HttpResponse.json({
          ...BASE_RESULT,
          rawEstimate: 0.5,
          biasCorrectedEstimate: 0.1, // bias = 0.4 > 0.1
        }),
      ),
    );
    render(
      <FeedbackLoopAlert experimentId="11111111-1111-1111-1111-111111111111" />,
    );
    const alert = await screen.findByTestId('feedback-loop-alert');
    expect(alert).toHaveAttribute('data-severity', 'ERROR');
  });

  it('renders nothing when server returns 404', async () => {
    server.use(
      http.post(`${ANALYSIS_SVC}/GetFeedbackLoopAnalysis`, () =>
        HttpResponse.json({ code: 'not_found' }, { status: 404 }),
      ),
    );
    const { container } = render(
      <FeedbackLoopAlert experimentId="00000000-0000-0000-0000-000000000000" />,
    );
    await new Promise((r) => setTimeout(r, 50));
    expect(container.firstChild).toBeNull();
  });

  it('renders nothing when contamination fraction is 0', async () => {
    server.use(
      http.post(`${ANALYSIS_SVC}/GetFeedbackLoopAnalysis`, () =>
        HttpResponse.json({ ...BASE_RESULT, contaminationFraction: 0 }),
      ),
    );
    const { container } = render(
      <FeedbackLoopAlert experimentId="11111111-1111-1111-1111-111111111111" />,
    );
    await new Promise((r) => setTimeout(r, 50));
    expect(container.firstChild).toBeNull();
  });

  it('uses explicit feedbackLoopDetected=false to suppress banner', async () => {
    server.use(
      http.post(`${ANALYSIS_SVC}/GetFeedbackLoopAnalysis`, () =>
        HttpResponse.json({ ...BASE_RESULT, feedbackLoopDetected: false }),
      ),
    );
    const { container } = render(
      <FeedbackLoopAlert experimentId="11111111-1111-1111-1111-111111111111" />,
    );
    await new Promise((r) => setTimeout(r, 50));
    expect(container.firstChild).toBeNull();
  });

  it('falls back to "primary metric" label when no primaryMetricId given', async () => {
    render(<FeedbackLoopAlert experimentId="11111111-1111-1111-1111-111111111111" />);
    const el = await screen.findByTestId('alert-metric-name');
    expect(el).toHaveTextContent('primary metric');
  });
});

// ---------------------------------------------------------------------------
// InterferenceTimelineChart
// ---------------------------------------------------------------------------
describe('InterferenceTimelineChart', () => {
  it('renders chart title and description', () => {
    render(<InterferenceTimelineChart result={BASE_RESULT} />);
    expect(screen.getByText('Treatment Effect Timeline')).toBeInTheDocument();
    expect(screen.getByText(/Treatment effect over time/)).toBeInTheDocument();
  });

  it('renders the accessible chart container', () => {
    render(<InterferenceTimelineChart result={BASE_RESULT} />);
    expect(
      screen.getByRole('img', { name: /Treatment effect over time with model retraining events/ }),
    ).toBeInTheDocument();
  });

  it('shows retrain event count footer', () => {
    render(<InterferenceTimelineChart result={BASE_RESULT} />);
    expect(screen.getByText(/2 retraining events marked/)).toBeInTheDocument();
  });

  it('returns null when prePostComparison is empty', () => {
    const emptyResult: FeedbackLoopResult = {
      ...BASE_RESULT,
      prePostComparison: [],
    };
    const { container } = render(<InterferenceTimelineChart result={emptyResult} />);
    expect(container.firstChild).toBeNull();
  });

  it('shows singular "event" for one retrain', () => {
    const oneEvent: FeedbackLoopResult = {
      ...BASE_RESULT,
      retrainingEvents: [BASE_RESULT.retrainingEvents[0]],
    };
    render(<InterferenceTimelineChart result={oneEvent} />);
    expect(screen.getByText(/1 retraining event marked/)).toBeInTheDocument();
  });
});
