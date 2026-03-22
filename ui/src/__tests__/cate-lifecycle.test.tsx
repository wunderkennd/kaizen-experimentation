import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import ResultsPage from '@/app/experiments/[id]/results/page';

let mockExperimentId = '11111111-1111-1111-1111-111111111111';

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

// Mock recharts
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

describe('Lifecycle Segments Tab - homepage_recs_v2', () => {
  beforeEach(() => {
    mockExperimentId = '11111111-1111-1111-1111-111111111111';
  });

  it('shows Lifecycle Segments tab for AB experiment', async () => {
    render(<ResultsPage />);

    await waitFor(() => {
      expect(screen.getByRole('tab', { name: 'Lifecycle Segments' })).toBeInTheDocument();
    });
  });

  it('shows heterogeneity detection banner with Cochran Q', async () => {
    const user = userEvent.setup();
    render(<ResultsPage />);

    await waitFor(() => {
      expect(screen.getByRole('tab', { name: 'Lifecycle Segments' })).toBeInTheDocument();
    });

    await user.click(screen.getByRole('tab', { name: 'Lifecycle Segments' }));

    await waitFor(() => {
      expect(screen.getByText('Heterogeneous Treatment Effects Detected')).toBeInTheDocument();
    });

    // Cochran Q = 12.4
    expect(screen.getByText(/Cochran Q = 12.4/)).toBeInTheDocument();
    // I² = 75.8% appears in banner and summary card
    expect(screen.getAllByText(/75.8%/).length).toBeGreaterThanOrEqual(1);
  });

  it('shows global ATE summary', async () => {
    const user = userEvent.setup();
    render(<ResultsPage />);

    await waitFor(() => {
      expect(screen.getByRole('tab', { name: 'Lifecycle Segments' })).toBeInTheDocument();
    });

    await user.click(screen.getByRole('tab', { name: 'Lifecycle Segments' }));

    await waitFor(() => {
      // Global ATE = +0.0140
      expect(screen.getByText('+0.0140')).toBeInTheDocument();
    });
  });

  it('shows subgroup effects table with segment names', async () => {
    const user = userEvent.setup();
    render(<ResultsPage />);

    await waitFor(() => {
      expect(screen.getByRole('tab', { name: 'Lifecycle Segments' })).toBeInTheDocument();
    });

    await user.click(screen.getByRole('tab', { name: 'Lifecycle Segments' }));

    await waitFor(() => {
      expect(screen.getByText('Trial')).toBeInTheDocument();
    });

    // All 4 segments present
    expect(screen.getByText('New (<30d)')).toBeInTheDocument();
    expect(screen.getByText('Established (30-180d)')).toBeInTheDocument();
    expect(screen.getByText('Mature (>180d)')).toBeInTheDocument();
  });

  it('shows significance badges for subgroups', async () => {
    const user = userEvent.setup();
    render(<ResultsPage />);

    await waitFor(() => {
      expect(screen.getByRole('tab', { name: 'Lifecycle Segments' })).toBeInTheDocument();
    });

    await user.click(screen.getByRole('tab', { name: 'Lifecycle Segments' }));

    await waitFor(() => {
      // TRIAL and NEW are significant, ESTABLISHED and MATURE are not
      const yesBadges = screen.getAllByText('Yes');
      const noBadges = screen.getAllByText('No');
      expect(yesBadges.length).toBe(2);
      expect(noBadges.length).toBe(2);
    });
  });

  it('shows BH FDR correction note', async () => {
    const user = userEvent.setup();
    render(<ResultsPage />);

    await waitFor(() => {
      expect(screen.getByRole('tab', { name: 'Lifecycle Segments' })).toBeInTheDocument();
    });

    await user.click(screen.getByRole('tab', { name: 'Lifecycle Segments' }));

    await waitFor(() => {
      expect(screen.getByText(/Benjamini-Hochberg FDR/)).toBeInTheDocument();
    });
  });

  it('shows forest plot section', async () => {
    const user = userEvent.setup();
    render(<ResultsPage />);

    await waitFor(() => {
      expect(screen.getByRole('tab', { name: 'Lifecycle Segments' })).toBeInTheDocument();
    });

    await user.click(screen.getByRole('tab', { name: 'Lifecycle Segments' }));

    await waitFor(() => {
      expect(screen.getByText('Lifecycle Segment Forest Plot')).toBeInTheDocument();
    });
  });
});

describe('Lifecycle Segments Tab - no data', () => {
  it('shows empty state for experiment without CATE data', async () => {
    mockExperimentId = '33333333-3333-3333-3333-333333333333';
    const user = userEvent.setup();
    render(<ResultsPage />);

    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'Results Dashboard' })).toBeInTheDocument();
    });

    // search_ranking_interleave is INTERLEAVING type — no lifecycle tab
    expect(screen.queryByRole('tab', { name: 'Lifecycle Segments' })).not.toBeInTheDocument();
  });

  it('shows empty state when CATE data not available for AB experiment', async () => {
    // thumbnail_selection_v1 is AB but has no CATE seed data
    mockExperimentId = '66666666-6666-6666-6666-666666666666';
    const user = userEvent.setup();
    render(<ResultsPage />);

    await waitFor(() => {
      expect(screen.getByRole('tab', { name: 'Lifecycle Segments' })).toBeInTheDocument();
    });

    await user.click(screen.getByRole('tab', { name: 'Lifecycle Segments' }));

    await waitFor(() => {
      expect(screen.getByText('No lifecycle segment analysis available for this experiment.')).toBeInTheDocument();
    });
  });
});
