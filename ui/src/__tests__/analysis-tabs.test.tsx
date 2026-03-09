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

describe('Analysis Tabs - Tab Navigation', () => {
  beforeEach(() => {
    mockExperimentId = '11111111-1111-1111-1111-111111111111';
  });

  it('renders tab buttons for all analysis views', async () => {
    render(<ResultsPage />);

    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'Results Dashboard' })).toBeInTheDocument();
    });

    expect(screen.getByRole('tab', { name: 'Overview' })).toBeInTheDocument();
    expect(screen.getByRole('tab', { name: 'Novelty Effects' })).toBeInTheDocument();
    expect(screen.getByRole('tab', { name: 'Content Interference' })).toBeInTheDocument();
    expect(screen.getByRole('tab', { name: 'Interleaving' })).toBeInTheDocument();
  });

  it('defaults to Overview tab showing treatment effects', async () => {
    render(<ResultsPage />);

    await waitFor(() => {
      expect(screen.getByText('Metric Results')).toBeInTheDocument();
    });

    expect(screen.getByRole('tab', { name: 'Overview' })).toHaveAttribute('aria-selected', 'true');
  });

  it('switches to Novelty tab when clicked', async () => {
    const user = userEvent.setup();
    render(<ResultsPage />);

    await waitFor(() => {
      expect(screen.getByRole('tab', { name: 'Novelty Effects' })).toBeInTheDocument();
    });

    await user.click(screen.getByRole('tab', { name: 'Novelty Effects' }));

    await waitFor(() => {
      expect(screen.getByText('Novelty Effect Detected')).toBeInTheDocument();
    });
  });

  it('switches to Interference tab when clicked', async () => {
    const user = userEvent.setup();
    render(<ResultsPage />);

    await waitFor(() => {
      expect(screen.getByRole('tab', { name: 'Content Interference' })).toBeInTheDocument();
    });

    await user.click(screen.getByRole('tab', { name: 'Content Interference' }));

    await waitFor(() => {
      expect(screen.getByText('Content Interference Detected')).toBeInTheDocument();
    });
  });
});

describe('Novelty Tab - homepage_recs_v2', () => {
  beforeEach(() => {
    mockExperimentId = '11111111-1111-1111-1111-111111111111';
  });

  it('shows novelty detection banner with metric name', async () => {
    const user = userEvent.setup();
    render(<ResultsPage />);

    await waitFor(() => {
      expect(screen.getByRole('tab', { name: 'Novelty Effects' })).toBeInTheDocument();
    });

    await user.click(screen.getByRole('tab', { name: 'Novelty Effects' }));

    await waitFor(() => {
      expect(screen.getByText('Novelty Effect Detected')).toBeInTheDocument();
    });

    expect(screen.getByText('click_through_rate')).toBeInTheDocument();
  });

  it('shows current effect and steady-state projection', async () => {
    const user = userEvent.setup();
    render(<ResultsPage />);

    await waitFor(() => {
      expect(screen.getByRole('tab', { name: 'Novelty Effects' })).toBeInTheDocument();
    });

    await user.click(screen.getByRole('tab', { name: 'Novelty Effects' }));

    await waitFor(() => {
      expect(screen.getByText('+0.0140')).toBeInTheDocument();
    });

    // Steady-state projection
    expect(screen.getByText('+0.0090')).toBeInTheDocument();
  });

  it('shows decay constant and stability status', async () => {
    const user = userEvent.setup();
    render(<ResultsPage />);

    await waitFor(() => {
      expect(screen.getByRole('tab', { name: 'Novelty Effects' })).toBeInTheDocument();
    });

    await user.click(screen.getByRole('tab', { name: 'Novelty Effects' }));

    await waitFor(() => {
      expect(screen.getByText('4.2 days')).toBeInTheDocument();
    });

    expect(screen.getByText('~6 days until projected stability')).toBeInTheDocument();
  });
});

describe('Interference Tab - homepage_recs_v2', () => {
  beforeEach(() => {
    mockExperimentId = '11111111-1111-1111-1111-111111111111';
  });

  it('shows interference detection banner', async () => {
    const user = userEvent.setup();
    render(<ResultsPage />);

    await waitFor(() => {
      expect(screen.getByRole('tab', { name: 'Content Interference' })).toBeInTheDocument();
    });

    await user.click(screen.getByRole('tab', { name: 'Content Interference' }));

    await waitFor(() => {
      expect(screen.getByText('Content Interference Detected')).toBeInTheDocument();
    });
  });

  it('shows JS divergence and Jaccard similarity', async () => {
    const user = userEvent.setup();
    render(<ResultsPage />);

    await waitFor(() => {
      expect(screen.getByRole('tab', { name: 'Content Interference' })).toBeInTheDocument();
    });

    await user.click(screen.getByRole('tab', { name: 'Content Interference' }));

    await waitFor(() => {
      expect(screen.getByText('0.0420')).toBeInTheDocument();
    });

    // Jaccard similarity 73%
    expect(screen.getByText('73.0%')).toBeInTheDocument();
  });

  it('shows spillover titles table', async () => {
    const user = userEvent.setup();
    render(<ResultsPage />);

    await waitFor(() => {
      expect(screen.getByRole('tab', { name: 'Content Interference' })).toBeInTheDocument();
    });

    await user.click(screen.getByRole('tab', { name: 'Content Interference' }));

    await waitFor(() => {
      expect(screen.getByText('Spillover Titles (3)')).toBeInTheDocument();
    });

    expect(screen.getByText('title-1234')).toBeInTheDocument();
    expect(screen.getByText('title-5678')).toBeInTheDocument();
    expect(screen.getByText('title-9012')).toBeInTheDocument();
  });

  it('shows Gini coefficient comparison', async () => {
    const user = userEvent.setup();
    render(<ResultsPage />);

    await waitFor(() => {
      expect(screen.getByRole('tab', { name: 'Content Interference' })).toBeInTheDocument();
    });

    await user.click(screen.getByRole('tab', { name: 'Content Interference' }));

    await waitFor(() => {
      expect(screen.getByText('Concentration (Gini Coefficient)')).toBeInTheDocument();
    });

    // Treatment Gini 0.61, Control Gini 0.58
    expect(screen.getByText('0.61')).toBeInTheDocument();
    expect(screen.getByText('0.58')).toBeInTheDocument();
  });
});

describe('Interleaving Tab - search_ranking_interleave', () => {
  beforeEach(() => {
    mockExperimentId = '33333333-3333-3333-3333-333333333333';
  });

  it('switches to Interleaving tab and shows sign test result', async () => {
    const user = userEvent.setup();
    render(<ResultsPage />);

    await waitFor(() => {
      expect(screen.getByRole('tab', { name: 'Interleaving' })).toBeInTheDocument();
    });

    await user.click(screen.getByRole('tab', { name: 'Interleaving' }));

    await waitFor(() => {
      expect(screen.getByText(/Sign Test: p = 0.003/)).toBeInTheDocument();
    });

    expect(screen.getByText('Significant difference detected between algorithms.')).toBeInTheDocument();
  });

  it('shows Bradley-Terry strength estimates', async () => {
    const user = userEvent.setup();
    render(<ResultsPage />);

    await waitFor(() => {
      expect(screen.getByRole('tab', { name: 'Interleaving' })).toBeInTheDocument();
    });

    await user.click(screen.getByRole('tab', { name: 'Interleaving' }));

    await waitFor(() => {
      expect(screen.getByText('Bradley-Terry Strength Estimates')).toBeInTheDocument();
    });

    // Algorithm names appear in both the strengths table and position header
    expect(screen.getAllByText('bm25_baseline').length).toBeGreaterThanOrEqual(1);
    expect(screen.getAllByText('semantic_search').length).toBeGreaterThanOrEqual(1);
    expect(screen.getByText('0.450')).toBeInTheDocument();
    expect(screen.getByText('0.550')).toBeInTheDocument();
  });

  it('shows position engagement rates table', async () => {
    const user = userEvent.setup();
    render(<ResultsPage />);

    await waitFor(() => {
      expect(screen.getByRole('tab', { name: 'Interleaving' })).toBeInTheDocument();
    });

    await user.click(screen.getByRole('tab', { name: 'Interleaving' }));

    await waitFor(() => {
      expect(screen.getByText('Position Engagement Rates')).toBeInTheDocument();
    });

    // Position #1
    expect(screen.getByText('#1')).toBeInTheDocument();
    // bm25 position 1 rate = 31.0%
    expect(screen.getByText('31.0%')).toBeInTheDocument();
    // semantic position 1 rate = 38.0%
    expect(screen.getByText('38.0%')).toBeInTheDocument();
  });
});

describe('Analysis Tabs - No Data States', () => {
  it('shows empty state for novelty tab when no data', async () => {
    mockExperimentId = '33333333-3333-3333-3333-333333333333';
    const user = userEvent.setup();
    render(<ResultsPage />);

    await waitFor(() => {
      expect(screen.getByRole('tab', { name: 'Novelty Effects' })).toBeInTheDocument();
    });

    await user.click(screen.getByRole('tab', { name: 'Novelty Effects' }));

    await waitFor(() => {
      expect(screen.getByText('No novelty analysis available for this experiment.')).toBeInTheDocument();
    });
  });

  it('shows empty state for interleaving tab when no data', async () => {
    mockExperimentId = '11111111-1111-1111-1111-111111111111';
    const user = userEvent.setup();
    render(<ResultsPage />);

    await waitFor(() => {
      expect(screen.getByRole('tab', { name: 'Interleaving' })).toBeInTheDocument();
    });

    await user.click(screen.getByRole('tab', { name: 'Interleaving' }));

    await waitFor(() => {
      expect(screen.getByText('No interleaving analysis available for this experiment.')).toBeInTheDocument();
    });
  });
});
