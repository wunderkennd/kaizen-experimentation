import { render, screen, waitFor } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import BanditDashboardPage from '@/app/experiments/[id]/bandit/page';

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

describe('Bandit Dashboard - error state', () => {
  it('shows error for non-bandit experiment', async () => {
    mockExperimentId = '11111111-1111-1111-1111-111111111111';
    render(<BanditDashboardPage />);

    await waitFor(() => {
      expect(screen.getByText(/no bandit dashboard/i)).toBeInTheDocument();
    });
  });
});

