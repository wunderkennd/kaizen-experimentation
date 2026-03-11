import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import ExperimentDetailPage from '@/app/experiments/[id]/page';
import { AuthProvider } from '@/lib/auth-context';
import type { AuthUser } from '@/lib/auth-context';

const defaultUser: AuthUser = { email: 'test@streamco.com', role: 'admin' };

function renderDetail(user: AuthUser = defaultUser) {
  return render(
    <AuthProvider initialUser={user}>
      <ExperimentDetailPage />
    </AuthProvider>,
  );
}

// Mutable ref to control which experiment ID is returned
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

// Mock recharts to avoid ResizeObserver errors from LayerAllocationChart
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
    Tooltip: Noop,
    Cell: Noop,
  };
});

describe('Experiment Detail Page', () => {
  beforeEach(() => {
    mockExperimentId = '11111111-1111-1111-1111-111111111111';
  });

  it('renders experiment name and description', async () => {
    renderDetail();

    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'homepage_recs_v2' })).toBeInTheDocument();
    });

    expect(
      screen.getByText('Test new recommendation algorithm on homepage carousel'),
    ).toBeInTheDocument();
  });

  it('renders state and type badges', async () => {
    renderDetail();

    await waitFor(() => {
      expect(screen.getByText('Running')).toBeInTheDocument();
    });

    expect(screen.getByText('A/B Test')).toBeInTheDocument();
  });

  it('renders variant table with control badge', async () => {
    renderDetail();

    await waitFor(() => {
      expect(screen.getByText('control')).toBeInTheDocument();
    });

    expect(screen.getByText('neural_recs')).toBeInTheDocument();
    expect(screen.getByText('Control')).toBeInTheDocument();
    const variantsSection = screen.getByText('Variants').closest('section')!;
    expect(within(variantsSection).getAllByText('50.0%')).toHaveLength(2);
  });

  it('renders metadata fields', async () => {
    renderDetail();

    await waitFor(() => {
      expect(screen.getByText('alice@streamco.com')).toBeInTheDocument();
    });

    expect(screen.getByText('click_through_rate')).toBeInTheDocument();
  });

  it('renders guardrails', async () => {
    renderDetail();

    await waitFor(() => {
      expect(screen.getByText('crash_rate')).toBeInTheDocument();
    });

    expect(screen.getByText('Auto-Pause')).toBeInTheDocument();
  });

  it('renders breadcrumb with link back to list', async () => {
    renderDetail();

    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'homepage_recs_v2' })).toBeInTheDocument();
    });

    const breadcrumbLink = screen.getByText('Experiments');
    expect(breadcrumbLink.closest('a')).toHaveAttribute('href', '/');
  });

  it('shows read-only variant table for RUNNING experiment', async () => {
    renderDetail();

    await waitFor(() => {
      expect(screen.getByText('control')).toBeInTheDocument();
    });

    // Should NOT have editable name inputs
    expect(screen.queryByDisplayValue('control')).not.toBeInTheDocument();
    // Should show the read-only table with traffic percentages (scoped to Variants section,
    // since LayerAllocationChart also renders 50.0% for bucket ranges)
    const variantsSection = screen.getByText('Variants').closest('section')!;
    expect(within(variantsSection).getAllByText('50.0%')).toHaveLength(2);
  });

  it('shows "Conclude Experiment" button for RUNNING experiment', async () => {
    renderDetail();

    await waitFor(() => {
      expect(screen.getByText('Conclude Experiment')).toBeInTheDocument();
    });
  });
});

describe('Experiment Detail Page - DRAFT experiment', () => {
  beforeEach(() => {
    mockExperimentId = '22222222-2222-2222-2222-222222222222';
  });

  it('shows variant editing form for DRAFT experiment', async () => {
    renderDetail();

    await waitFor(() => {
      expect(screen.getByDisplayValue('control')).toBeInTheDocument();
    });

    expect(screen.getByDisplayValue('ml_abr')).toBeInTheDocument();
    expect(screen.getByText('Save Variants')).toBeInTheDocument();
    expect(screen.getByText('Add Variant')).toBeInTheDocument();
  });

  it('shows "Start Experiment" button for DRAFT experiment', async () => {
    renderDetail();

    await waitFor(() => {
      expect(screen.getByText('Start Experiment')).toBeInTheDocument();
    });
  });

  it('transitions DRAFT to RUNNING on start', async () => {
    const user = userEvent.setup();
    renderDetail();

    await waitFor(() => {
      expect(screen.getByText('Start Experiment')).toBeInTheDocument();
    });

    await user.click(screen.getByText('Start Experiment'));
    await user.click(screen.getByText('Start'));

    await waitFor(() => {
      expect(screen.getByText('Running')).toBeInTheDocument();
    });
  });
});

describe('Experiment Detail Page - STARTING experiment', () => {
  beforeEach(() => {
    mockExperimentId = '55555555-5555-5555-5555-555555555555';
  });

  it('shows starting checklist for STARTING experiment', async () => {
    renderDetail();

    await waitFor(() => {
      expect(screen.getByText('Starting Experiment')).toBeInTheDocument();
    });

    expect(screen.getByText('Configuration validated')).toBeInTheDocument();
    expect(screen.getByText('Traffic ramp in progress')).toBeInTheDocument();
  });
});

describe('Experiment Detail Page - CONCLUDING experiment', () => {
  beforeEach(() => {
    mockExperimentId = '66666666-6666-6666-6666-666666666666';
  });

  it('shows concluding progress for CONCLUDING experiment', async () => {
    renderDetail();

    await waitFor(() => {
      expect(screen.getByText('Concluding Experiment')).toBeInTheDocument();
    });

    expect(screen.getByText('Stopping traffic')).toBeInTheDocument();
    expect(screen.getByText('Running final analysis')).toBeInTheDocument();
    expect(screen.getByText('Generating report')).toBeInTheDocument();
  });
});
