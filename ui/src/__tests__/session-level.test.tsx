import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import ResultsPage from '@/app/experiments/[id]/results/page';
import { SessionLevelTab } from '@/components/session-level-tab';
import type { MetricResult } from '@/lib/types';

let mockExperimentId = 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa';

vi.mock('next/navigation', () => ({
  useParams: () => ({ id: mockExperimentId }),
  useRouter: () => ({ push: vi.fn(), replace: vi.fn() }),
  useSearchParams: () => new URLSearchParams(),
}));

vi.mock('next/link', () => ({
  default: ({ children, href, ...props }: { children: React.ReactNode; href: string; [key: string]: unknown }) => (
    <a href={href} {...props}>{children}</a>
  ),
}));

vi.mock('next/dynamic', () => ({
  default: (loader: () => Promise<{ default: React.ComponentType<unknown> }>) => {
    let Comp: React.ComponentType<unknown> | null = null;
    loader().then((mod) => { Comp = mod.default; });
    return function DynamicMock(props: Record<string, unknown>) {
      return Comp ? <Comp {...props} /> : null;
    };
  },
}));

vi.mock('recharts', async () => {
  const Passthrough = ({ children }: { children?: React.ReactNode }) => (
    <div data-testid="responsive-container">{children}</div>
  );
  const Noop = () => null;

  return {
    ResponsiveContainer: Passthrough,
    ComposedChart: Passthrough,
    BarChart: Passthrough,
    Bar: Noop,
    Scatter: Noop,
    Line: Noop,
    Area: Noop,
    XAxis: Noop,
    YAxis: Noop,
    CartesianGrid: Noop,
    ReferenceLine: Noop,
    Tooltip: Noop,
    ErrorBar: Noop,
    Cell: Noop,
    Legend: Noop,
  };
});

const makeMetric = (overrides: Partial<MetricResult> & { metricId: string }): MetricResult => ({
  variantId: 'v-treatment',
  controlMean: 100,
  treatmentMean: 110,
  absoluteEffect: 10,
  relativeEffect: 0.1,
  ciLower: 2,
  ciUpper: 18,
  pValue: 0.01,
  isSignificant: true,
  cupedAdjustedEffect: 0,
  cupedCiLower: 0,
  cupedCiUpper: 0,
  varianceReductionPct: 0,
  ...overrides,
});

describe('SessionLevelTab component', () => {
  it('renders info banner with explanation text', () => {
    const metrics: MetricResult[] = [
      makeMetric({
        metricId: 'test_metric',
        sessionLevelResult: {
          naiveSe: 10, clusteredSe: 14.5, designEffect: 2.1,
          naivePValue: 0.01, clusteredPValue: 0.04,
        },
      }),
    ];

    render(<SessionLevelTab metricResults={metrics} />);

    expect(screen.getByText('Session-Level Analysis')).toBeInTheDocument();
    expect(screen.getByText(/HC1-clustered standard errors/)).toBeInTheDocument();
  });

  it('renders comparison table with correct columns', () => {
    const metrics: MetricResult[] = [
      makeMetric({
        metricId: 'watch_time',
        sessionLevelResult: {
          naiveSe: 48.2, clusteredSe: 69.8, designEffect: 2.1,
          naivePValue: 0.003, clusteredPValue: 0.039,
        },
      }),
    ];

    render(<SessionLevelTab metricResults={metrics} />);

    expect(screen.getByText('Metric')).toBeInTheDocument();
    expect(screen.getByText('Naive SE')).toBeInTheDocument();
    expect(screen.getByText('Clustered SE')).toBeInTheDocument();
    expect(screen.getByText('Design Effect')).toBeInTheDocument();
    expect(screen.getByText('Naive p-value')).toBeInTheDocument();
    expect(screen.getByText('Clustered p-value')).toBeInTheDocument();
    expect(screen.getByText('Significance Shift')).toBeInTheDocument();
  });

  it('shows "Low" green badge for design effect <= 1.5', () => {
    const metrics: MetricResult[] = [
      makeMetric({
        metricId: 'low_de_metric',
        sessionLevelResult: {
          naiveSe: 10, clusteredSe: 11, designEffect: 1.2,
          naivePValue: 0.01, clusteredPValue: 0.02,
        },
      }),
    ];

    render(<SessionLevelTab metricResults={metrics} />);

    const badges = screen.getAllByText(/Low/);
    const tableBadge = badges.find(el => el.className.includes('bg-green-100'));
    expect(tableBadge).toBeDefined();
  });

  it('shows "Moderate" yellow badge for design effect 1.5–3.0', () => {
    const metrics: MetricResult[] = [
      makeMetric({
        metricId: 'mod_de_metric',
        sessionLevelResult: {
          naiveSe: 48.2, clusteredSe: 69.8, designEffect: 2.1,
          naivePValue: 0.003, clusteredPValue: 0.039,
        },
      }),
    ];

    render(<SessionLevelTab metricResults={metrics} />);

    const badges = screen.getAllByText(/Moderate/);
    const tableBadge = badges.find(el => el.className.includes('bg-yellow-100'));
    expect(tableBadge).toBeDefined();
  });

  it('shows "High" red badge for design effect > 3.0', () => {
    const metrics: MetricResult[] = [
      makeMetric({
        metricId: 'high_de_metric',
        sessionLevelResult: {
          naiveSe: 10, clusteredSe: 20, designEffect: 4.0,
          naivePValue: 0.01, clusteredPValue: 0.08,
        },
      }),
    ];

    render(<SessionLevelTab metricResults={metrics} />);

    const badges = screen.getAllByText(/High/);
    const tableBadge = badges.find(el => el.className.includes('bg-red-100'));
    expect(tableBadge).toBeDefined();
  });

  it('shows significance shift warning when naive significant but clustered not', () => {
    const metrics: MetricResult[] = [
      makeMetric({
        metricId: 'shift_metric',
        sessionLevelResult: {
          naiveSe: 0.15, clusteredSe: 0.171, designEffect: 1.3,
          naivePValue: 0.046, clusteredPValue: 0.079,
        },
      }),
    ];

    render(<SessionLevelTab metricResults={metrics} />);

    expect(screen.getByText('Under-estimated')).toBeInTheDocument();
  });

  it('does not show shift warning when both agree', () => {
    const metrics: MetricResult[] = [
      makeMetric({
        metricId: 'agree_metric',
        sessionLevelResult: {
          naiveSe: 48.2, clusteredSe: 69.8, designEffect: 2.1,
          naivePValue: 0.003, clusteredPValue: 0.039,
        },
      }),
    ];

    render(<SessionLevelTab metricResults={metrics} />);

    expect(screen.queryByText('Under-estimated')).not.toBeInTheDocument();
  });

  it('shows empty state when no metrics have session-level data', () => {
    render(<SessionLevelTab metricResults={[]} />);

    expect(screen.getByText('No session-level analysis available for this experiment.')).toBeInTheDocument();
  });
});

describe('Session-Level tab in results page', () => {
  beforeEach(() => {
    mockExperimentId = 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa';
  });

  it('shows Session-Level tab for SESSION_LEVEL experiment', async () => {
    render(<ResultsPage />);

    await waitFor(() => {
      expect(screen.getByRole('tab', { name: 'Session-Level' })).toBeInTheDocument();
    });
  });

  it('shows session-level data when tab is clicked', async () => {
    const user = userEvent.setup();
    render(<ResultsPage />);

    await waitFor(() => {
      expect(screen.getByRole('tab', { name: 'Session-Level' })).toBeInTheDocument();
    });

    await user.click(screen.getByRole('tab', { name: 'Session-Level' }));

    await waitFor(() => {
      expect(screen.getByText('Session-Level Analysis')).toBeInTheDocument();
    });

    // Verify watch_time_per_session row with design effect 2.1
    expect(screen.getByText('watch_time_per_session')).toBeInTheDocument();
    expect(screen.getAllByText(/Moderate/).length).toBeGreaterThanOrEqual(1);
  });

  it('hides Session-Level tab for experiments without session data', async () => {
    mockExperimentId = '11111111-1111-1111-1111-111111111111';
    render(<ResultsPage />);

    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'Results Dashboard' })).toBeInTheDocument();
    });

    expect(screen.queryByRole('tab', { name: 'Session-Level' })).not.toBeInTheDocument();
  });
});
