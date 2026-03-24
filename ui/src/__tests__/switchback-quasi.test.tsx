import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import ResultsPage from '@/app/experiments/[id]/results/page';

let mockExperimentId = 'eeeeeeee-eeee-eeee-eeee-eeeeeeeeeeee';

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
    AreaChart: Passthrough,
    Area: Noop,
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

describe('Switchback Tab — delivery_speed_switchback_v1', () => {
  beforeEach(() => {
    mockExperimentId = 'eeeeeeee-eeee-eeee-eeee-eeeeeeeeeeee';
  });

  it('shows Switchback Blocks tab for SWITCHBACK experiment', async () => {
    render(<ResultsPage />);

    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'Results Dashboard' })).toBeInTheDocument();
    });

    expect(screen.getByRole('tab', { name: 'Switchback Blocks' })).toBeInTheDocument();
  });

  it('does not show Synthetic Control tab for SWITCHBACK experiment', async () => {
    render(<ResultsPage />);

    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'Results Dashboard' })).toBeInTheDocument();
    });

    expect(screen.queryByRole('tab', { name: 'Synthetic Control' })).not.toBeInTheDocument();
  });

  it('switches to Switchback Blocks tab and shows ATE estimate', async () => {
    const user = userEvent.setup();
    render(<ResultsPage />);

    await waitFor(() => {
      expect(screen.getByRole('tab', { name: 'Switchback Blocks' })).toBeInTheDocument();
    });

    await user.click(screen.getByRole('tab', { name: 'Switchback Blocks' }));

    await waitFor(() => {
      // ATE = 0.0155, formatted as +0.0155
      expect(screen.getByText('+0.0155')).toBeInTheDocument();
    });
  });

  it('shows block count summary (6T / 6C)', async () => {
    const user = userEvent.setup();
    render(<ResultsPage />);

    await waitFor(() => {
      expect(screen.getByRole('tab', { name: 'Switchback Blocks' })).toBeInTheDocument();
    });

    await user.click(screen.getByRole('tab', { name: 'Switchback Blocks' }));

    await waitFor(() => {
      expect(screen.getByText('6T / 6C')).toBeInTheDocument();
    });
  });

  it('shows RI p-value', async () => {
    const user = userEvent.setup();
    render(<ResultsPage />);

    await waitFor(() => {
      expect(screen.getByRole('tab', { name: 'Switchback Blocks' })).toBeInTheDocument();
    });

    await user.click(screen.getByRole('tab', { name: 'Switchback Blocks' }));

    await waitFor(() => {
      // formatPValue(0.031) → '0.03' (p >= 0.01 → toFixed(2))
      expect(screen.getAllByText('0.03').length).toBeGreaterThan(0);
    });
  });

  it('shows block-level outcome table', async () => {
    const user = userEvent.setup();
    render(<ResultsPage />);

    await waitFor(() => {
      expect(screen.getByRole('tab', { name: 'Switchback Blocks' })).toBeInTheDocument();
    });

    await user.click(screen.getByRole('tab', { name: 'Switchback Blocks' }));

    await waitFor(() => {
      expect(screen.getByText('Block-Level Outcomes')).toBeInTheDocument();
    });

    // Verify first block ID appears
    expect(screen.getByText('blk-01')).toBeInTheDocument();
  });

  it('shows block timeline', async () => {
    const user = userEvent.setup();
    render(<ResultsPage />);

    await waitFor(() => {
      expect(screen.getByRole('tab', { name: 'Switchback Blocks' })).toBeInTheDocument();
    });

    await user.click(screen.getByRole('tab', { name: 'Switchback Blocks' }));

    await waitFor(() => {
      expect(screen.getByText('Block Timeline')).toBeInTheDocument();
    });

    expect(screen.getByText('12 blocks total')).toBeInTheDocument();
  });

  it('shows ACF carryover diagnostic chart', async () => {
    const user = userEvent.setup();
    render(<ResultsPage />);

    await waitFor(() => {
      expect(screen.getByRole('tab', { name: 'Switchback Blocks' })).toBeInTheDocument();
    });

    await user.click(screen.getByRole('tab', { name: 'Switchback Blocks' }));

    await waitFor(() => {
      expect(screen.getByText('ACF — Carryover Diagnostic')).toBeInTheDocument();
    });
  });

  it('shows RI null distribution histogram', async () => {
    const user = userEvent.setup();
    render(<ResultsPage />);

    await waitFor(() => {
      expect(screen.getByRole('tab', { name: 'Switchback Blocks' })).toBeInTheDocument();
    });

    await user.click(screen.getByRole('tab', { name: 'Switchback Blocks' }));

    await waitFor(() => {
      expect(screen.getByText('Randomization Inference — Null Distribution')).toBeInTheDocument();
    });
  });

  it('shows empty state when no switchback data for experiment', async () => {
    mockExperimentId = '11111111-1111-1111-1111-111111111111';
    const user = userEvent.setup();
    render(<ResultsPage />);

    // homepage_recs_v2 is AB, so Switchback Blocks tab should not appear
    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'Results Dashboard' })).toBeInTheDocument();
    });

    expect(screen.queryByRole('tab', { name: 'Switchback Blocks' })).not.toBeInTheDocument();
  });
});

describe('Quasi-Experiment Tab — market_expansion_synthetic_control', () => {
  beforeEach(() => {
    mockExperimentId = 'dddddddd-dddd-dddd-dddd-dddddddddddd';
  });

  it('shows Synthetic Control tab for QUASI_EXPERIMENT', async () => {
    render(<ResultsPage />);

    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'Results Dashboard' })).toBeInTheDocument();
    });

    expect(screen.getByRole('tab', { name: 'Synthetic Control' })).toBeInTheDocument();
  });

  it('does not show Switchback Blocks tab for QUASI_EXPERIMENT', async () => {
    render(<ResultsPage />);

    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'Results Dashboard' })).toBeInTheDocument();
    });

    expect(screen.queryByRole('tab', { name: 'Switchback Blocks' })).not.toBeInTheDocument();
  });

  it('shows RMSPE diagnostic badge with ratio', async () => {
    const user = userEvent.setup();
    render(<ResultsPage />);

    await waitFor(() => {
      expect(screen.getByRole('tab', { name: 'Synthetic Control' })).toBeInTheDocument();
    });

    await user.click(screen.getByRole('tab', { name: 'Synthetic Control' }));

    await waitFor(() => {
      expect(screen.getByText(/RMSPE Diagnostic/)).toBeInTheDocument();
    });

    // rmspeRatio = 4.90 → "Poor Fit"
    expect(screen.getByText(/Poor Fit/)).toBeInTheDocument();
  });

  it('shows pre and post RMSPE values', async () => {
    const user = userEvent.setup();
    render(<ResultsPage />);

    await waitFor(() => {
      expect(screen.getByRole('tab', { name: 'Synthetic Control' })).toBeInTheDocument();
    });

    await user.click(screen.getByRole('tab', { name: 'Synthetic Control' }));

    await waitFor(() => {
      expect(screen.getAllByText('Pre-RMSPE').length).toBeGreaterThan(0);
    });

    // preRmspe = 0.0029 and postRmspe = 0.0142 appear in badge + summary cards
    expect(screen.getAllByText('0.0029').length).toBeGreaterThan(0);
    expect(screen.getAllByText('0.0142').length).toBeGreaterThan(0);
  });

  it('shows p-value in significant badge', async () => {
    const user = userEvent.setup();
    render(<ResultsPage />);

    await waitFor(() => {
      expect(screen.getByRole('tab', { name: 'Synthetic Control' })).toBeInTheDocument();
    });

    await user.click(screen.getByRole('tab', { name: 'Synthetic Control' }));

    await waitFor(() => {
      // formatPValue(0.033) → '0.03' (p >= 0.01 → toFixed(2))
      expect(screen.getByText('p = 0.03')).toBeInTheDocument();
    });
  });

  it('shows treated vs synthetic control chart section', async () => {
    const user = userEvent.setup();
    render(<ResultsPage />);

    await waitFor(() => {
      expect(screen.getByRole('tab', { name: 'Synthetic Control' })).toBeInTheDocument();
    });

    await user.click(screen.getByRole('tab', { name: 'Synthetic Control' }));

    await waitFor(() => {
      expect(screen.getByText('Treated vs Synthetic Control')).toBeInTheDocument();
    });
  });

  it('shows pointwise and cumulative effects charts', async () => {
    const user = userEvent.setup();
    render(<ResultsPage />);

    await waitFor(() => {
      expect(screen.getByRole('tab', { name: 'Synthetic Control' })).toBeInTheDocument();
    });

    await user.click(screen.getByRole('tab', { name: 'Synthetic Control' }));

    await waitFor(() => {
      expect(screen.getByText('Pointwise Treatment Effects')).toBeInTheDocument();
    });

    expect(screen.getByText('Cumulative Treatment Effect')).toBeInTheDocument();
  });

  it('shows donor weights table with top donor', async () => {
    const user = userEvent.setup();
    render(<ResultsPage />);

    await waitFor(() => {
      expect(screen.getByRole('tab', { name: 'Synthetic Control' })).toBeInTheDocument();
    });

    await user.click(screen.getByRole('tab', { name: 'Synthetic Control' }));

    await waitFor(() => {
      expect(screen.getByText('Donor Weights')).toBeInTheDocument();
    });

    // Seattle Metro appears in donor table and placebo grid
    expect(screen.getAllByText('Seattle Metro').length).toBeGreaterThan(0);
    expect(screen.getByText('0.4200')).toBeInTheDocument();
  });

  it('shows placebo small-multiples grid', async () => {
    const user = userEvent.setup();
    render(<ResultsPage />);

    await waitFor(() => {
      expect(screen.getByRole('tab', { name: 'Synthetic Control' })).toBeInTheDocument();
    });

    await user.click(screen.getByRole('tab', { name: 'Synthetic Control' }));

    await waitFor(() => {
      expect(screen.getByText(/Placebo Tests \(5 donors\)/)).toBeInTheDocument();
    });

    // All 5 donors should appear (placebo grid + donor weight table)
    expect(screen.getAllByText('Portland').length).toBeGreaterThan(0);
    expect(screen.getAllByText('Denver').length).toBeGreaterThan(0);
  });

  it('shows empty state when no synthetic control data', async () => {
    mockExperimentId = '11111111-1111-1111-1111-111111111111';
    const user = userEvent.setup();
    render(<ResultsPage />);

    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'Results Dashboard' })).toBeInTheDocument();
    });

    // homepage_recs_v2 is AB, not QUASI_EXPERIMENT
    expect(screen.queryByRole('tab', { name: 'Synthetic Control' })).not.toBeInTheDocument();
  });
});
