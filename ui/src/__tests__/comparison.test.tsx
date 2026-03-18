import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi } from 'vitest';
import ComparePage from '@/app/compare/page';

vi.mock('next/navigation', () => ({
  useParams: () => ({}),
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
    BarChart: Passthrough,
    Bar: Noop,
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

describe('Experiment Comparison Page', () => {
  it('renders page heading and experiment selector', async () => {
    render(<ComparePage />);

    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'Experiment Comparison' })).toBeInTheDocument();
    });

    expect(screen.getByTestId('experiment-search')).toBeInTheDocument();
  });

  it('shows empty state when no experiments selected', async () => {
    render(<ComparePage />);

    await waitFor(() => {
      expect(screen.getByTestId('empty-state')).toBeInTheDocument();
    });

    expect(screen.getByText('No experiments selected')).toBeInTheDocument();
    expect(screen.getByText(/Select 2 or more experiments/)).toBeInTheDocument();
  });

  it('selector shows available experiments (RUNNING/CONCLUDED)', async () => {
    const user = userEvent.setup();
    render(<ComparePage />);

    await waitFor(() => {
      expect(screen.getByTestId('experiment-search')).toBeInTheDocument();
    });

    // Click on search input to open dropdown
    const searchInput = screen.getByTestId('experiment-search');
    await user.click(searchInput);

    await waitFor(() => {
      expect(screen.getByTestId('experiment-dropdown')).toBeInTheDocument();
    });

    // Should show RUNNING and CONCLUDED experiments
    // RUNNING: homepage_recs_v2, search_ranking_interleave, recommendation_holdout_q1
    // CONCLUDED: retention_nudge_v1, session_watch_pattern
    expect(screen.getByText('homepage_recs_v2')).toBeInTheDocument();
    expect(screen.getByText('search_ranking_interleave')).toBeInTheDocument();
    expect(screen.getByText('retention_nudge_v1')).toBeInTheDocument();
    expect(screen.getByText('session_watch_pattern')).toBeInTheDocument();

    // Should NOT show DRAFT experiments
    expect(screen.queryByText('adaptive_bitrate_v3')).not.toBeInTheDocument();
    expect(screen.queryByText('cold_start_bandit')).not.toBeInTheDocument();
  });

  it('selecting experiments fetches and displays results', async () => {
    const user = userEvent.setup();
    render(<ComparePage />);

    await waitFor(() => {
      expect(screen.getByTestId('experiment-search')).toBeInTheDocument();
    });

    // Select first experiment
    const searchInput = screen.getByTestId('experiment-search');
    await user.click(searchInput);

    await waitFor(() => {
      expect(screen.getByText('homepage_recs_v2')).toBeInTheDocument();
    });

    await user.click(screen.getByText('homepage_recs_v2'));

    // Select second experiment
    await user.click(searchInput);

    await waitFor(() => {
      expect(screen.getByText('retention_nudge_v1')).toBeInTheDocument();
    });

    await user.click(screen.getByText('retention_nudge_v1'));

    // Should display comparison tables once both loaded
    await waitFor(() => {
      expect(screen.getByTestId('metadata-table')).toBeInTheDocument();
    });

    expect(screen.getByTestId('primary-metric-table')).toBeInTheDocument();
  });

  it('comparison table shows metric results side-by-side', async () => {
    const user = userEvent.setup();
    render(<ComparePage />);

    await waitFor(() => {
      expect(screen.getByTestId('experiment-search')).toBeInTheDocument();
    });

    // Select two experiments
    const searchInput = screen.getByTestId('experiment-search');
    await user.click(searchInput);
    await waitFor(() => { expect(screen.getByText('homepage_recs_v2')).toBeInTheDocument(); });
    await user.click(screen.getByText('homepage_recs_v2'));

    await user.click(searchInput);
    await waitFor(() => { expect(screen.getByText('retention_nudge_v1')).toBeInTheDocument(); });
    await user.click(screen.getByText('retention_nudge_v1'));

    // Wait for comparison tables to render
    await waitFor(() => {
      expect(screen.getByTestId('primary-metric-table')).toBeInTheDocument();
    });

    // Check primary metric names are displayed (may appear in multiple tables)
    expect(screen.getAllByText('click_through_rate').length).toBeGreaterThanOrEqual(1);
    expect(screen.getAllByText('day_7_retention').length).toBeGreaterThanOrEqual(1);

    // Check treatment effects are shown
    expect(screen.getAllByText('+0.0140').length).toBeGreaterThanOrEqual(1); // homepage_recs_v2 CTR effect
    expect(screen.getAllByText('+0.0500').length).toBeGreaterThanOrEqual(1); // retention_nudge_v1 effect

    // Check SRM status
    const okBadges = screen.getAllByText('OK');
    expect(okBadges.length).toBe(2); // Both experiments have no SRM mismatch
  });

  it('chart renders with effect sizes', async () => {
    const user = userEvent.setup();
    render(<ComparePage />);

    await waitFor(() => {
      expect(screen.getByTestId('experiment-search')).toBeInTheDocument();
    });

    // Select two experiments
    const searchInput = screen.getByTestId('experiment-search');
    await user.click(searchInput);
    await waitFor(() => { expect(screen.getByText('homepage_recs_v2')).toBeInTheDocument(); });
    await user.click(screen.getByText('homepage_recs_v2'));

    await user.click(searchInput);
    await waitFor(() => { expect(screen.getByText('retention_nudge_v1')).toBeInTheDocument(); });
    await user.click(screen.getByText('retention_nudge_v1'));

    // Wait for chart to render
    await waitFor(() => {
      expect(screen.getByTestId('comparison-chart')).toBeInTheDocument();
    });

    expect(screen.getByText('Effect Size Comparison')).toBeInTheDocument();
    expect(screen.getAllByTestId('responsive-container').length).toBeGreaterThanOrEqual(1);
  });

  it('metric alignment matrix shows shared metrics', async () => {
    const user = userEvent.setup();
    render(<ComparePage />);

    await waitFor(() => {
      expect(screen.getByTestId('experiment-search')).toBeInTheDocument();
    });

    // Select homepage_recs_v2 and thumbnail_selection_v1 (CONCLUDING — not shown)
    // Instead select homepage_recs_v2 and search_ranking_interleave (both RUNNING)
    const searchInput = screen.getByTestId('experiment-search');
    await user.click(searchInput);
    await waitFor(() => { expect(screen.getByText('homepage_recs_v2')).toBeInTheDocument(); });
    await user.click(screen.getByText('homepage_recs_v2'));

    await user.click(searchInput);
    await waitFor(() => { expect(screen.getByText('search_ranking_interleave')).toBeInTheDocument(); });
    await user.click(screen.getByText('search_ranking_interleave'));

    // Wait for metric alignment table
    await waitFor(() => {
      expect(screen.getByTestId('metric-alignment-table')).toBeInTheDocument();
    });

    expect(screen.getByText('Metric Alignment Matrix')).toBeInTheDocument();

    // Both should have their own metrics shown as checkmarks
    // homepage_recs_v2 has: click_through_rate, watch_time_per_session, content_diversity_score
    // search_ranking_interleave has: search_success_rate, clicks_per_search
    // No shared metrics between these two
    const checkmarks = screen.getAllByText('\u2713');
    expect(checkmarks.length).toBeGreaterThanOrEqual(2);
  });

  it('can remove an experiment from comparison', async () => {
    const user = userEvent.setup();
    render(<ComparePage />);

    await waitFor(() => {
      expect(screen.getByTestId('experiment-search')).toBeInTheDocument();
    });

    // Select two experiments
    const searchInput = screen.getByTestId('experiment-search');
    await user.click(searchInput);
    await waitFor(() => { expect(screen.getByText('homepage_recs_v2')).toBeInTheDocument(); });
    await user.click(screen.getByText('homepage_recs_v2'));

    await user.click(searchInput);
    await waitFor(() => { expect(screen.getByText('retention_nudge_v1')).toBeInTheDocument(); });
    await user.click(screen.getByText('retention_nudge_v1'));

    // Wait for comparison tables
    await waitFor(() => {
      expect(screen.getByTestId('metadata-table')).toBeInTheDocument();
    });

    // Remove homepage_recs_v2
    const removeButton = screen.getByLabelText('Remove homepage_recs_v2');
    await user.click(removeButton);

    // Should show "select more" message since only 1 experiment remains
    await waitFor(() => {
      expect(screen.getByTestId('select-more')).toBeInTheDocument();
    });

    expect(screen.getByText(/Select at least one more experiment/)).toBeInTheDocument();
  });
});
