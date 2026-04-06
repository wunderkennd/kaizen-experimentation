/**
 * Sprint 5.4: Portfolio + Meta-Experiment + Slate Heatmap UI tests.
 *
 * Coverage:
 *   - WinRateChart: renders lines, empty state
 *   - LearningRateChart: renders lines, empty state
 *   - AnnualizedImpactChart: renders bars with error bars, empty state
 *   - ParetoFrontierPlot: renders scatter, frontier/dominated, empty state
 *   - MetaResultsPanel: loads data, renders table, winner badge, cochran Q
 *   - SlateHeatmap: loads data, renders grid cells, empty state
 *   - Portfolio page: renders new chart sections
 *   - Experiment detail page: meta-results-tab, slate heatmap integration
 */

import { render, screen, waitFor, within } from '@testing-library/react';
import { http, HttpResponse } from 'msw';
import { server } from '@/__mocks__/server';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import React from 'react';

import { WinRateChart } from '@/components/charts/win-rate-chart';
import { LearningRateChart } from '@/components/charts/learning-rate-chart';
import { AnnualizedImpactChart } from '@/components/charts/annualized-impact-chart';
import { ParetoFrontierPlot } from '@/components/charts/pareto-frontier-plot';
import { SlateHeatmap } from '@/components/slate/SlateHeatmap';
import { MetaResultsPanel } from '@/components/meta/MetaResultsPanel';
import {
  SEED_PORTFOLIO_METRICS,
  SEED_PARETO_FRONTIER,
  SEED_META_EXPERIMENT_RESULTS,
  SEED_SLATE_HEATMAP_RESULTS,
  SEED_PORTFOLIO_ALLOCATION,
} from '@/__mocks__/seed-data';

const ANALYSIS_SVC = '*/experimentation.analysis.v1.AnalysisService';
const MGMT_SVC = '*/experimentation.management.v1.ExperimentManagementService';

// Mock recharts for unit tests — passthrough containers, noop chart internals
vi.mock('recharts', async () => {
  const Passthrough = ({ children }: { children?: React.ReactNode }) => (
    <div data-testid="responsive-container">{children}</div>
  );
  const Noop = () => null;

  return {
    ResponsiveContainer: Passthrough,
    LineChart: Passthrough,
    BarChart: Passthrough,
    ScatterChart: Passthrough,
    Line: Noop,
    Bar: Noop,
    Scatter: Noop,
    XAxis: Noop,
    YAxis: Noop,
    ZAxis: Noop,
    CartesianGrid: Noop,
    Tooltip: Noop,
    Legend: Noop,
    Cell: Noop,
    ErrorBar: Noop,
  };
});

// ---------------------------------------------------------------------------
// WinRateChart
// ---------------------------------------------------------------------------

describe('WinRateChart', () => {
  it('renders chart with data', () => {
    render(<WinRateChart data={SEED_PORTFOLIO_METRICS.winRates} />);
    expect(screen.getByTestId('win-rate-chart')).toBeInTheDocument();
    expect(screen.getByText('Win Rate Over Time')).toBeInTheDocument();
  });

  it('shows empty state when no data', () => {
    render(<WinRateChart data={[]} />);
    expect(screen.getByText(/no win rate data/i)).toBeInTheDocument();
  });

  it('renders responsive container', () => {
    render(<WinRateChart data={SEED_PORTFOLIO_METRICS.winRates} />);
    expect(screen.getAllByTestId('responsive-container').length).toBeGreaterThan(0);
  });
});

// ---------------------------------------------------------------------------
// LearningRateChart
// ---------------------------------------------------------------------------

describe('LearningRateChart', () => {
  it('renders chart with data', () => {
    render(<LearningRateChart data={SEED_PORTFOLIO_METRICS.learningRates} />);
    expect(screen.getByTestId('learning-rate-chart')).toBeInTheDocument();
    expect(screen.getByText('Learning Rate')).toBeInTheDocument();
  });

  it('shows empty state when no data', () => {
    render(<LearningRateChart data={[]} />);
    expect(screen.getByText(/no learning rate data/i)).toBeInTheDocument();
  });

  it('shows description text', () => {
    render(<LearningRateChart data={SEED_PORTFOLIO_METRICS.learningRates} />);
    expect(screen.getByText(/information accumulation rate/i)).toBeInTheDocument();
  });
});

// ---------------------------------------------------------------------------
// AnnualizedImpactChart
// ---------------------------------------------------------------------------

describe('AnnualizedImpactChart', () => {
  it('renders chart with data', () => {
    render(<AnnualizedImpactChart data={SEED_PORTFOLIO_METRICS.annualizedImpacts} />);
    expect(screen.getByTestId('annualized-impact-chart')).toBeInTheDocument();
    expect(screen.getByText('Annualized Impact')).toBeInTheDocument();
  });

  it('shows empty state when no data', () => {
    render(<AnnualizedImpactChart data={[]} />);
    expect(screen.getByText(/no annualized impact data/i)).toBeInTheDocument();
  });

  it('shows description text', () => {
    render(<AnnualizedImpactChart data={SEED_PORTFOLIO_METRICS.annualizedImpacts} />);
    expect(screen.getByText(/projected yearly effect size/i)).toBeInTheDocument();
  });
});

// ---------------------------------------------------------------------------
// ParetoFrontierPlot
// ---------------------------------------------------------------------------

describe('ParetoFrontierPlot', () => {
  it('renders with data points', () => {
    render(
      <ParetoFrontierPlot
        points={SEED_PARETO_FRONTIER.points}
        frontierIds={SEED_PARETO_FRONTIER.frontierIds}
      />,
    );
    expect(screen.getByTestId('pareto-frontier-plot')).toBeInTheDocument();
    expect(screen.getByText('Pareto Frontier')).toBeInTheDocument();
  });

  it('shows empty state when no points', () => {
    render(<ParetoFrontierPlot points={[]} frontierIds={[]} />);
    expect(screen.getByText(/no multi-objective data/i)).toBeInTheDocument();
  });

  it('shows legend with pareto and dominated counts', () => {
    render(
      <ParetoFrontierPlot
        points={SEED_PARETO_FRONTIER.points}
        frontierIds={SEED_PARETO_FRONTIER.frontierIds}
      />,
    );
    // 3 pareto optimal, 2 dominated
    expect(screen.getByText(/Pareto optimal \(3\)/)).toBeInTheDocument();
    expect(screen.getByText(/Dominated \(2\)/)).toBeInTheDocument();
  });

  it('shows trade-off description', () => {
    render(
      <ParetoFrontierPlot
        points={SEED_PARETO_FRONTIER.points}
        frontierIds={SEED_PARETO_FRONTIER.frontierIds}
      />,
    );
    expect(screen.getByText(/multi-objective trade-offs/i)).toBeInTheDocument();
  });
});

// ---------------------------------------------------------------------------
// MetaResultsPanel
// ---------------------------------------------------------------------------

const META_EXP_ID = 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa';

describe('MetaResultsPanel', () => {
  it('renders loading state initially', () => {
    render(<MetaResultsPanel experimentId={META_EXP_ID} />);
    expect(screen.getByRole('status', { name: /loading meta results/i })).toBeInTheDocument();
  });

  it('renders results table after data loads', async () => {
    render(<MetaResultsPanel experimentId={META_EXP_ID} />);
    await waitFor(() => {
      expect(screen.getByTestId('meta-results-panel')).toBeInTheDocument();
    });
    expect(screen.getByTestId('meta-results-table')).toBeInTheDocument();
  });

  it('shows overall winner badge', async () => {
    render(<MetaResultsPanel experimentId={META_EXP_ID} />);
    await waitFor(() => {
      expect(screen.getByTestId('overall-winner')).toBeInTheDocument();
    });
    expect(screen.getByTestId('overall-winner')).toHaveTextContent('treatment');
  });

  it('shows variant rows with policy types', async () => {
    render(<MetaResultsPanel experimentId={META_EXP_ID} />);
    await waitFor(() => {
      expect(screen.getByTestId('meta-variant-row-v-ctrl')).toBeInTheDocument();
    });
    expect(screen.getByTestId('meta-variant-row-v-treat')).toBeInTheDocument();
    expect(screen.getByText('Thompson Sampling')).toBeInTheDocument();
    expect(screen.getByText('Linear UCB')).toBeInTheDocument();
  });

  it('shows Cochran Q p-value with significance badge', async () => {
    render(<MetaResultsPanel experimentId={META_EXP_ID} />);
    await waitFor(() => {
      expect(screen.getByText(/0\.0320/)).toBeInTheDocument();
    });
    expect(screen.getByTestId('significance-badge')).toBeInTheDocument();
  });

  it('shows best arm names', async () => {
    render(<MetaResultsPanel experimentId={META_EXP_ID} />);
    await waitFor(() => {
      expect(screen.getByText('arm-a')).toBeInTheDocument();
    });
    expect(screen.getByText('arm-x')).toBeInTheDocument();
  });

  it('shows computed-at timestamp', async () => {
    render(<MetaResultsPanel experimentId={META_EXP_ID} />);
    await waitFor(() => {
      expect(screen.getByTestId('meta-computed-at')).toBeInTheDocument();
    });
  });

  it('shows error state for unknown experiment', async () => {
    render(<MetaResultsPanel experimentId="00000000-0000-0000-0000-000000000000" />);
    await waitFor(() => {
      expect(screen.getByRole('button', { name: /retry/i })).toBeInTheDocument();
    });
  });
});

// ---------------------------------------------------------------------------
// SlateHeatmap
// ---------------------------------------------------------------------------

const SLATE_EXP_ID = 'cccccccc-cccc-cccc-cccc-cccccccccccc';

describe('SlateHeatmap', () => {
  it('renders loading state initially', () => {
    render(<SlateHeatmap experimentId={SLATE_EXP_ID} />);
    expect(screen.getByRole('status', { name: /loading slate heatmap/i })).toBeInTheDocument();
  });

  it('renders heatmap grid after data loads', async () => {
    render(<SlateHeatmap experimentId={SLATE_EXP_ID} />);
    await waitFor(() => {
      expect(screen.getByTestId('slate-heatmap')).toBeInTheDocument();
    });
    expect(screen.getByText('Slate Assignment Heatmap')).toBeInTheDocument();
  });

  it('renders cells for each item/position combination', async () => {
    render(<SlateHeatmap experimentId={SLATE_EXP_ID} />);
    await waitFor(() => {
      expect(screen.getByTestId('heatmap-cell-item-a-1')).toBeInTheDocument();
    });
    // Check specific high-probability cell
    expect(screen.getByTestId('heatmap-cell-item-e-5')).toBeInTheDocument();
  });

  it('shows description text', async () => {
    render(<SlateHeatmap experimentId={SLATE_EXP_ID} />);
    await waitFor(() => {
      expect(screen.getByText(/assignment probability per item/i)).toBeInTheDocument();
    });
  });

  it('shows computed-at timestamp', async () => {
    render(<SlateHeatmap experimentId={SLATE_EXP_ID} />);
    await waitFor(() => {
      expect(screen.getByTestId('heatmap-computed-at')).toBeInTheDocument();
    });
  });

  it('shows error state for unknown experiment', async () => {
    render(<SlateHeatmap experimentId="00000000-0000-0000-0000-000000000000" />);
    await waitFor(() => {
      expect(screen.getByRole('button', { name: /retry/i })).toBeInTheDocument();
    });
  });
});

// ---------------------------------------------------------------------------
// Portfolio Dashboard integration (new charts appear)
// ---------------------------------------------------------------------------

vi.mock('next/navigation', () => ({
  useParams: () => ({}),
  useRouter: () => ({ push: vi.fn(), replace: vi.fn() }),
  useSearchParams: () => new URLSearchParams(),
  usePathname: () => '/portfolio',
}));

vi.mock('next/link', () => ({
  default: ({ children, href, ...props }: { children: React.ReactNode; href: string; [key: string]: unknown }) => (
    <a href={href} {...props}>{children}</a>
  ),
}));

// Mock dynamic imports for portfolio page charts
vi.mock('next/dynamic', () => ({
  default: (fn: () => Promise<{ default: React.ComponentType<Record<string, unknown>> }>) => {
    let Resolved: React.ComponentType<Record<string, unknown>> | null = null;
    const promise = fn();
    promise.then((mod) => { Resolved = mod.default; });
    return function DynamicMock(props: Record<string, unknown>) {
      if (Resolved) return <Resolved {...props} />;
      // For budget-allocation-chart, return a stub with data-testid
      const experiments = props.experiments as Array<{ experimentId: string }> | undefined;
      if (experiments !== undefined) {
        return <div data-testid="budget-allocation-chart" data-experiment-count={experiments?.length ?? 0} />;
      }
      // For other charts, return a placeholder div
      return <div data-testid="dynamic-chart-stub" />;
    };
  },
}));

import PortfolioDashboard from '@/app/portfolio/page';

describe('Portfolio Dashboard — Sprint 5.4 charts', () => {
  it('renders page heading', async () => {
    render(<PortfolioDashboard />);
    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'Portfolio Dashboard', level: 1 })).toBeInTheDocument();
    });
  });

  it('renders the budget allocation chart', async () => {
    render(<PortfolioDashboard />);
    await waitFor(() => {
      expect(screen.getByTestId('budget-allocation-chart')).toBeInTheDocument();
    });
  });

  it('renders win rate chart section', async () => {
    render(<PortfolioDashboard />);
    await waitFor(() => {
      expect(screen.getByText('Win Rate Over Time')).toBeInTheDocument();
    });
  });

  it('renders learning rate chart section', async () => {
    render(<PortfolioDashboard />);
    await waitFor(() => {
      expect(screen.getByText('Learning Rate')).toBeInTheDocument();
    });
  });

  it('renders annualized impact chart section', async () => {
    render(<PortfolioDashboard />);
    await waitFor(() => {
      expect(screen.getByText('Annualized Impact')).toBeInTheDocument();
    });
  });

  it('renders Pareto frontier plot section', async () => {
    render(<PortfolioDashboard />);
    await waitFor(() => {
      expect(screen.getByText('Pareto Frontier')).toBeInTheDocument();
    });
  });

  it('still renders portfolio table', async () => {
    render(<PortfolioDashboard />);
    await waitFor(() => {
      expect(screen.getByTestId('portfolio-table')).toBeInTheDocument();
    });
  });

  it('gracefully handles metrics API failure', async () => {
    server.use(
      http.post(`${ANALYSIS_SVC}/GetPortfolioMetrics`, () => {
        return HttpResponse.json({ message: 'Internal server error' }, { status: 500 });
      }),
    );

    render(<PortfolioDashboard />);
    // Should still render the page with the allocation data
    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'Portfolio Dashboard', level: 1 })).toBeInTheDocument();
    });
    expect(screen.getByTestId('portfolio-table')).toBeInTheDocument();
  });
});
