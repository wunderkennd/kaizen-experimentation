import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import ResultsPage from '@/app/experiments/[id]/results/page';

let mockExperimentId = '11111111-1111-1111-1111-111111111111';

vi.mock('next/navigation', () => ({
  useParams: () => ({ id: mockExperimentId }),
  useRouter: () => ({ push: vi.fn() }),
}));

vi.mock('next/link', () => ({
  default: ({ children, href, ...props }: { children: React.ReactNode; href: string; [key: string]: unknown }) => (
    <a href={href} {...props}>{children}</a>
  ),
}));

// Mock next/dynamic to eagerly resolve dynamic imports in tests
vi.mock('next/dynamic', () => ({
  default: (loader: () => Promise<{ default: React.ComponentType<unknown> }>) => {
    let Comp: React.ComponentType<unknown> | null = null;
    loader().then((mod) => { Comp = mod.default; });
    return function DynamicMock(props: Record<string, unknown>) {
      return Comp ? <Comp {...props} /> : null;
    };
  },
}));

// Mock recharts to avoid SVG rendering issues in jsdom
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
    Line: Noop,
    Scatter: Noop,
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

describe('Results Dashboard - homepage_recs_v2 (experiment 111...)', () => {
  beforeEach(() => {
    mockExperimentId = '11111111-1111-1111-1111-111111111111';
  });

  it('shows loading then renders results', async () => {
    render(<ResultsPage />);

    // Loading spinner should be present initially
    expect(document.querySelector('.animate-spin')).toBeInTheDocument();

    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'Results Dashboard' })).toBeInTheDocument();
    });
  });

  it('renders treatment effects table with correct metric data', async () => {
    render(<ResultsPage />);

    await waitFor(() => {
      expect(screen.getAllByText('click_through_rate').length).toBeGreaterThanOrEqual(1);
    });

    expect(screen.getAllByText('watch_time_per_session').length).toBeGreaterThanOrEqual(1);
    expect(screen.getAllByText('content_diversity_score').length).toBeGreaterThanOrEqual(1);
  });

  it('shows significance indicators (green for significant, gray for not)', async () => {
    render(<ResultsPage />);

    await waitFor(() => {
      expect(screen.getAllByText('click_through_rate').length).toBeGreaterThanOrEqual(1);
    });

    const significantBadges = screen.getAllByText('Significant');
    const notSignificantBadges = screen.getAllByText('Not Significant');

    expect(significantBadges).toHaveLength(2);
    expect(notSignificantBadges).toHaveLength(1);
  });

  it('does NOT show SRM banner when no mismatch', async () => {
    render(<ResultsPage />);

    await waitFor(() => {
      expect(screen.getAllByText('click_through_rate').length).toBeGreaterThanOrEqual(1);
    });

    expect(screen.queryByText('Sample Ratio Mismatch Detected')).not.toBeInTheDocument();
  });

  it('CUPED toggle switches between raw and adjusted values', async () => {
    const user = userEvent.setup();
    render(<ResultsPage />);

    await waitFor(() => {
      expect(screen.getByText('CUPED Adjustment')).toBeInTheDocument();
    });

    // Default: raw values — click_through_rate effect is 0.014 → "+0.0140"
    expect(screen.getByText('+0.0140')).toBeInTheDocument();

    // Toggle CUPED on
    const toggle = screen.getByRole('switch');
    await user.click(toggle);

    // CUPED adjusted: click_through_rate effect is 0.013 → "+0.0130"
    expect(screen.getByText('+0.0130')).toBeInTheDocument();
  });

  it('shows variance reduction percentage near toggle', async () => {
    render(<ResultsPage />);

    await waitFor(() => {
      expect(screen.getByText('~32% variance reduction')).toBeInTheDocument();
    });
  });

  it('renders forest plot container', async () => {
    render(<ResultsPage />);

    await waitFor(() => {
      expect(screen.getByText('Treatment Effects')).toBeInTheDocument();
    });

    expect(screen.getAllByTestId('responsive-container').length).toBeGreaterThanOrEqual(1);
  });

  it('renders sequential boundary info for mSPRT experiment', async () => {
    render(<ResultsPage />);

    await waitFor(() => {
      expect(screen.getByText('Sequential Testing (Alpha Spending)')).toBeInTheDocument();
    });

    expect(screen.getByText('Boundary Crossed')).toBeInTheDocument();
  });

  it('breadcrumb links to correct URLs', async () => {
    render(<ResultsPage />);

    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'Results Dashboard' })).toBeInTheDocument();
    });

    const links = screen.getAllByText('Experiments');
    expect(links[0].closest('a')).toHaveAttribute('href', '/');

    const detailLinks = screen.getAllByText('Detail');
    expect(detailLinks[0].closest('a')).toHaveAttribute(
      'href',
      '/experiments/11111111-1111-1111-1111-111111111111',
    );
  });

  it('shows summary with significant metrics count', async () => {
    render(<ResultsPage />);

    await waitFor(() => {
      expect(screen.getByText('2 / 3')).toBeInTheDocument();
    });

    expect(screen.getByText('homepage_recs_v2')).toBeInTheDocument();
  });
});

describe('Results Dashboard - thumbnail_selection_v1 (experiment 666..., SRM mismatch)', () => {
  beforeEach(() => {
    mockExperimentId = '66666666-6666-6666-6666-666666666666';
  });

  it('shows SRM banner when mismatch detected', async () => {
    render(<ResultsPage />);

    await waitFor(() => {
      expect(screen.getByText('Sample Ratio Mismatch Detected')).toBeInTheDocument();
    });

    expect(screen.getByText(/14\.82/)).toBeInTheDocument();
    expect(screen.getByText(/< 0\.001/)).toBeInTheDocument();
  });
});

describe('Results Dashboard - error state', () => {
  beforeEach(() => {
    mockExperimentId = 'bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb';
  });

  it('shows error state for experiment without analysis data', async () => {
    render(<ResultsPage />);

    await waitFor(() => {
      expect(screen.getByText(/not found/i)).toBeInTheDocument();
    });
  });
});
