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

describe('Surrogate Tab - homepage_recs_v2', () => {
  beforeEach(() => {
    mockExperimentId = '11111111-1111-1111-1111-111111111111';
  });

  it('shows Surrogate Projections tab for experiment with surrogate data', async () => {
    render(<ResultsPage />);

    await waitFor(() => {
      expect(screen.getByRole('tab', { name: 'Surrogate Projections' })).toBeInTheDocument();
    });
  });

  it('shows surrogate projection table with R² badges', async () => {
    const user = userEvent.setup();
    render(<ResultsPage />);

    await waitFor(() => {
      expect(screen.getByRole('tab', { name: 'Surrogate Projections' })).toBeInTheDocument();
    });

    await user.click(screen.getByRole('tab', { name: 'Surrogate Projections' }));

    await waitFor(() => {
      expect(screen.getByText('monthly_retention_rate')).toBeInTheDocument();
    });

    // Surrogate metric name
    expect(screen.getByText('click_through_rate')).toBeInTheDocument();
    // R² = 0.78 -> High badge
    expect(screen.getByText('High')).toBeInTheDocument();
    // R² = 0.52 -> Medium badge
    expect(screen.getByText('Medium')).toBeInTheDocument();
  });

  it('shows projected effects for long-term metrics', async () => {
    const user = userEvent.setup();
    render(<ResultsPage />);

    await waitFor(() => {
      expect(screen.getByRole('tab', { name: 'Surrogate Projections' })).toBeInTheDocument();
    });

    await user.click(screen.getByRole('tab', { name: 'Surrogate Projections' }));

    await waitFor(() => {
      // monthly_retention_rate projected effect = 0.008
      expect(screen.getByText('+0.0080')).toBeInTheDocument();
    });

    // lifetime_value projected effect = 2.45
    expect(screen.getByText('+2.4500')).toBeInTheDocument();
  });

  it('shows calibration guide with color legend', async () => {
    const user = userEvent.setup();
    render(<ResultsPage />);

    await waitFor(() => {
      expect(screen.getByRole('tab', { name: 'Surrogate Projections' })).toBeInTheDocument();
    });

    await user.click(screen.getByRole('tab', { name: 'Surrogate Projections' }));

    await waitFor(() => {
      expect(screen.getByText('Calibration Guide')).toBeInTheDocument();
    });
  });
});

describe('Surrogate Tab - no surrogate data', () => {
  it('does not show Surrogate Projections tab for experiment without surrogates', async () => {
    mockExperimentId = '33333333-3333-3333-3333-333333333333';
    render(<ResultsPage />);

    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'Results Dashboard' })).toBeInTheDocument();
    });

    expect(screen.queryByRole('tab', { name: 'Surrogate Projections' })).not.toBeInTheDocument();
  });
});

describe('Guardrails Tab - homepage_recs_v2', () => {
  beforeEach(() => {
    mockExperimentId = '11111111-1111-1111-1111-111111111111';
  });

  it('shows Guardrails tab for experiment with guardrail configs', async () => {
    render(<ResultsPage />);

    await waitFor(() => {
      expect(screen.getByRole('tab', { name: 'Guardrails' })).toBeInTheDocument();
    });
  });

  it('shows breach history table with actions', async () => {
    const user = userEvent.setup();
    render(<ResultsPage />);

    await waitFor(() => {
      expect(screen.getByRole('tab', { name: 'Guardrails' })).toBeInTheDocument();
    });

    await user.click(screen.getByRole('tab', { name: 'Guardrails' }));

    await waitFor(() => {
      expect(screen.getAllByText('crash_rate').length).toBe(2);
    });

    // Breach values
    expect(screen.getByText('0.0120')).toBeInTheDocument();
    expect(screen.getByText('0.0140')).toBeInTheDocument();

    // Action badges
    expect(screen.getByText('Alert')).toBeInTheDocument();
    expect(screen.getByText('Auto-Pause')).toBeInTheDocument();
  });

  it('shows breach count and variant', async () => {
    const user = userEvent.setup();
    render(<ResultsPage />);

    await waitFor(() => {
      expect(screen.getByRole('tab', { name: 'Guardrails' })).toBeInTheDocument();
    });

    await user.click(screen.getByRole('tab', { name: 'Guardrails' }));

    await waitFor(() => {
      expect(screen.getAllByText('v1-treatment').length).toBe(2);
    });
  });
});

describe('Guardrails Tab - no guardrails', () => {
  it('does not show Guardrails tab for experiment without guardrail configs', async () => {
    mockExperimentId = '33333333-3333-3333-3333-333333333333';
    render(<ResultsPage />);

    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'Results Dashboard' })).toBeInTheDocument();
    });

    // search_ranking_interleave has empty guardrailConfigs
    expect(screen.queryByRole('tab', { name: 'Guardrails' })).not.toBeInTheDocument();
  });
});

describe('Holdout Tab - no holdout data', () => {
  it('does not show Holdout Lift tab for non-holdout experiment', async () => {
    mockExperimentId = '11111111-1111-1111-1111-111111111111';
    render(<ResultsPage />);

    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'Results Dashboard' })).toBeInTheDocument();
    });

    expect(screen.queryByRole('tab', { name: 'Holdout Lift' })).not.toBeInTheDocument();
  });
});
