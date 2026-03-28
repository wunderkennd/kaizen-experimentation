import { render, screen, waitFor } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import BanditDashboardPage from '@/app/experiments/[id]/bandit/page';
import { RewardCompositionChart } from '@/components/RewardCompositionChart';
import { ConstraintStatusTable } from '@/components/ConstraintStatusTable';
import type { ArmObjectiveBreakdown, RewardObjective, ConstraintStatus } from '@/lib/types';

let mockExperimentId = '44444444-4444-4444-4444-444444444444';

vi.mock('next/navigation', () => ({
  useParams: () => ({ id: mockExperimentId }),
  useRouter: () => ({ push: vi.fn() }),
}));

vi.mock('next/link', () => ({
  default: ({ children, href, ...props }: { children: React.ReactNode; href: string; [key: string]: unknown }) => (
    <a href={href} {...props}>{children}</a>
  ),
}));

// Mock recharts
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
    Tooltip: Noop,
    Cell: Noop,
    Legend: Noop,
  };
});

describe('Bandit Dashboard - cold_start_bandit (experiment 444...)', () => {
  beforeEach(() => {
    mockExperimentId = '44444444-4444-4444-4444-444444444444';
  });

  it('shows loading then renders dashboard', async () => {
    render(<BanditDashboardPage />);

    expect(document.querySelector('.animate-spin')).toBeInTheDocument();

    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'Bandit Dashboard' })).toBeInTheDocument();
    });
  });

  it('shows experiment name and algorithm in summary', async () => {
    render(<BanditDashboardPage />);

    await waitFor(() => {
      expect(screen.getByText('cold_start_bandit')).toBeInTheDocument();
    });

    expect(screen.getByText('THOMPSON SAMPLING')).toBeInTheDocument();
  });

  it('shows total rewards processed', async () => {
    render(<BanditDashboardPage />);

    await waitFor(() => {
      expect(screen.getByText('3,842')).toBeInTheDocument();
    });
  });

  it('shows Active status when not in warmup', async () => {
    render(<BanditDashboardPage />);

    await waitFor(() => {
      expect(screen.getByText('Active')).toBeInTheDocument();
    });
  });

  it('renders arm statistics table with all 4 arms', async () => {
    render(<BanditDashboardPage />);

    await waitFor(() => {
      expect(screen.getByText('Arm Statistics')).toBeInTheDocument();
    });

    expect(screen.getByText('top_carousel')).toBeInTheDocument();
    expect(screen.getByText('genre_row')).toBeInTheDocument();
    expect(screen.getByText('trending_section')).toBeInTheDocument();
    expect(screen.getByText('personalized_row')).toBeInTheDocument();
  });

  it('shows selection counts and reward rates', async () => {
    render(<BanditDashboardPage />);

    await waitFor(() => {
      expect(screen.getByText('Arm Statistics')).toBeInTheDocument();
    });

    // top_carousel: 1200 selections, 26.0% reward rate
    expect(screen.getAllByText('1,200').length).toBeGreaterThanOrEqual(1);
    // All reward rate values visible
    expect(screen.getAllByText('26.0%').length).toBeGreaterThanOrEqual(1);
  });

  it('shows Thompson Sampling alpha/beta columns', async () => {
    render(<BanditDashboardPage />);

    await waitFor(() => {
      expect(screen.getByText('Alpha')).toBeInTheDocument();
    });

    expect(screen.getByText('Beta')).toBeInTheDocument();
    // top_carousel alpha=313
    expect(screen.getByText('313')).toBeInTheDocument();
    // top_carousel beta=889
    expect(screen.getByText('889')).toBeInTheDocument();
  });

  it('renders allocation and reward rate chart sections', async () => {
    render(<BanditDashboardPage />);

    await waitFor(() => {
      expect(screen.getByText('Arm Allocation')).toBeInTheDocument();
    });

    expect(screen.getByText('Reward Rates')).toBeInTheDocument();
    expect(screen.getByText('Reward Rate Over Time')).toBeInTheDocument();
    expect(screen.getAllByTestId('responsive-container').length).toBeGreaterThanOrEqual(3);
  });

  it('shows min exploration floor', async () => {
    render(<BanditDashboardPage />);

    await waitFor(() => {
      expect(screen.getByText(/Min exploration floor: 10%/)).toBeInTheDocument();
    });
  });

  it('breadcrumb links to correct URLs', async () => {
    render(<BanditDashboardPage />);

    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'Bandit Dashboard' })).toBeInTheDocument();
    });

    const expLinks = screen.getAllByText('Experiments');
    expect(expLinks[0].closest('a')).toHaveAttribute('href', '/');

    const detailLinks = screen.getAllByText('Detail');
    expect(detailLinks[0].closest('a')).toHaveAttribute('href', '/experiments/44444444-4444-4444-4444-444444444444');
  });
});

describe('Bandit Dashboard - multi-objective reward composition (ADR-011)', () => {
  beforeEach(() => {
    mockExperimentId = '44444444-4444-4444-4444-444444444444';
  });

  it('renders reward composition section when objectives present', async () => {
    render(<BanditDashboardPage />);

    await waitFor(() => {
      expect(screen.getByText('Reward Composition per Arm')).toBeInTheDocument();
    });
  });

  it('renders LP constraint status section', async () => {
    render(<BanditDashboardPage />);

    await waitFor(() => {
      expect(screen.getByText('LP Constraint Status')).toBeInTheDocument();
    });
  });

  it('shows VIOLATED constraint with red badge', async () => {
    render(<BanditDashboardPage />);

    await waitFor(() => {
      expect(screen.getByText('VIOLATED')).toBeInTheDocument();
    });
  });

  it('shows SATISFIED constraint badge', async () => {
    render(<BanditDashboardPage />);

    await waitFor(() => {
      expect(screen.getByText('SATISFIED')).toBeInTheDocument();
    });
  });

  it('shows constraint labels from seed data', async () => {
    render(<BanditDashboardPage />);

    await waitFor(() => {
      expect(screen.getByText('max_single_provider_share')).toBeInTheDocument();
    });
    expect(screen.getByText('min_diversity_floor')).toBeInTheDocument();
  });
});

describe('RewardCompositionChart unit tests', () => {
  const objectives: RewardObjective[] = [
    { metricId: 'engagement', weight: 0.6, floor: 0.0, isPrimary: true },
    { metricId: 'diversity', weight: 0.4, floor: 0.3, isPrimary: false },
  ];

  const breakdowns: ArmObjectiveBreakdown[] = [
    { armId: 'arm-1', armName: 'Arm A', objectiveContributions: { engagement: 0.3, diversity: 0.2 }, composedReward: 0.5 },
    { armId: 'arm-2', armName: 'Arm B', objectiveContributions: { engagement: 0.2, diversity: 0.3 }, composedReward: 0.5 },
  ];

  it('renders chart container with objectives and breakdowns', () => {
    render(<RewardCompositionChart breakdowns={breakdowns} objectives={objectives} />);
    expect(screen.getByRole('img', { name: /multi-objective reward composition/i })).toBeInTheDocument();
  });

  it('shows primary objective label in footer', () => {
    render(<RewardCompositionChart breakdowns={breakdowns} objectives={objectives} />);
    expect(screen.getByText(/Primary objective/)).toBeInTheDocument();
    expect(screen.getAllByText(/engagement/).length).toBeGreaterThanOrEqual(1);
  });

  it('renders empty state when breakdowns is empty', () => {
    render(<RewardCompositionChart breakdowns={[]} objectives={objectives} />);
    expect(screen.getByText(/No objective breakdown data/)).toBeInTheDocument();
  });

  it('renders empty state when objectives is empty', () => {
    render(<RewardCompositionChart breakdowns={breakdowns} objectives={[]} />);
    expect(screen.getByText(/No objective breakdown data/)).toBeInTheDocument();
  });
});

describe('ConstraintStatusTable unit tests', () => {
  const constraints: ConstraintStatus[] = [
    { label: 'provider_cap', currentValue: 0.3, limit: 0.5, isSatisfied: true },
    { label: 'diversity_floor', currentValue: 0.15, limit: 0.2, isSatisfied: false },
  ];

  it('renders constraint labels and values', () => {
    render(<ConstraintStatusTable constraints={constraints} />);
    expect(screen.getByText('provider_cap')).toBeInTheDocument();
    expect(screen.getByText('diversity_floor')).toBeInTheDocument();
  });

  it('shows SATISFIED badge for satisfied constraints', () => {
    render(<ConstraintStatusTable constraints={constraints} />);
    expect(screen.getByText('SATISFIED')).toBeInTheDocument();
  });

  it('shows VIOLATED badge for violated constraints', () => {
    render(<ConstraintStatusTable constraints={constraints} />);
    expect(screen.getByText('VIOLATED')).toBeInTheDocument();
  });

  it('applies red row highlight on violated row', () => {
    const { container } = render(<ConstraintStatusTable constraints={constraints} />);
    const rows = container.querySelectorAll('tbody tr');
    expect(rows[0]).not.toHaveClass('bg-red-50');
    expect(rows[1]).toHaveClass('bg-red-50');
  });

  it('renders empty state when no constraints', () => {
    render(<ConstraintStatusTable constraints={[]} />);
    expect(screen.getByText(/No LP constraints configured/)).toBeInTheDocument();
  });

  it('shows current value and limit columns', () => {
    render(<ConstraintStatusTable constraints={constraints} />);
    expect(screen.getByText('Current Value')).toBeInTheDocument();
    expect(screen.getByText('Limit')).toBeInTheDocument();
  });
});

describe('Bandit Dashboard - error state', () => {
  it('shows error for non-bandit experiment', async () => {
    mockExperimentId = '11111111-1111-1111-1111-111111111111';
    render(<BanditDashboardPage />);

    await waitFor(() => {
      expect(screen.getByText(/no bandit dashboard/i)).toBeInTheDocument();
    });
  });
});

