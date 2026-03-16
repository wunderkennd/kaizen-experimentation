import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { http, HttpResponse } from 'msw';
import { server } from '@/__mocks__/server';
import { clearApiCache } from '@/lib/api';
import { AuthProvider } from '@/lib/auth-context';
import ResultsPage from '@/app/experiments/[id]/results/page';
import ExperimentListPage from '@/app/page';
import ExperimentDetailPage from '@/app/experiments/[id]/page';

const defaultUser = { email: 'test@streamco.com', role: 'experimenter' as const };

let mockExperimentId = '11111111-1111-1111-1111-111111111111';

vi.mock('next/navigation', () => ({
  useParams: () => ({ id: mockExperimentId }),
  useRouter: () => ({ push: vi.fn(), replace: vi.fn() }),
  useSearchParams: () => new URLSearchParams(),
}));

vi.mock('@/lib/toast-context', () => ({
  useToast: () => ({ addToast: vi.fn(), removeToast: vi.fn(), toasts: [] }),
  ToastProvider: ({ children }: { children: React.ReactNode }) => children,
}));

vi.mock('next/link', () => ({
  default: ({ children, href, ...props }: { children: React.ReactNode; href: string; [key: string]: unknown }) => (
    <a href={href} {...props}>{children}</a>
  ),
}));

vi.mock('next/dynamic', () => ({
  default: (loader: () => Promise<{ default: React.ComponentType<unknown> }>) => {
    let Comp: React.ComponentType<unknown> | null = null;
    loader().then((mod) => { Comp = mod.default; });
    return function DynamicMock(props: Record<string, unknown>) {
      return Comp ? <Comp {...props} /> : null;
    };
  },
}));

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
    Area: Noop,
  };
});

const MGMT_SVC = '*/experimentation.management.v1.ExperimentManagementService';
const ANALYSIS_SVC = '*/experimentation.analysis.v1.AnalysisService';

describe('Chaos: Full backend outage on list page', () => {
  it('shows retryable error when ListExperiments returns 500', async () => {
    server.use(
      http.post(`${MGMT_SVC}/ListExperiments`, () => {
        return HttpResponse.json(
          { code: 'internal', message: 'Service unavailable' },
          { status: 500 },
        );
      }),
    );

    render(<AuthProvider initialUser={defaultUser}><ExperimentListPage /></AuthProvider>);

    await waitFor(() => {
      expect(screen.getByTestId('retryable-error')).toBeInTheDocument();
    });

    expect(screen.getByTestId('retry-button')).toBeInTheDocument();
    expect(screen.getByText(/Service unavailable/)).toBeInTheDocument();
  });

  it('recovers when backend comes back after retry', async () => {
    const user = userEvent.setup();

    // Start with backend down
    server.use(
      http.post(`${MGMT_SVC}/ListExperiments`, () => {
        return HttpResponse.json(
          { code: 'internal', message: 'Service unavailable' },
          { status: 500 },
        );
      }),
    );

    render(<AuthProvider initialUser={defaultUser}><ExperimentListPage /></AuthProvider>);

    await waitFor(() => {
      expect(screen.getByTestId('retryable-error')).toBeInTheDocument();
    });

    // Restore backend — remove the override so the default handler responds
    server.resetHandlers();
    clearApiCache();

    await user.click(screen.getByTestId('retry-button'));

    await waitFor(() => {
      expect(screen.getByText('homepage_recs_v2')).toBeInTheDocument();
    });

    expect(screen.queryByTestId('retryable-error')).not.toBeInTheDocument();
  });
});

describe('Chaos: Detail page backend failure', () => {
  beforeEach(() => {
    mockExperimentId = '11111111-1111-1111-1111-111111111111';
  });

  it('shows retryable error when GetExperiment returns 500', async () => {
    server.use(
      http.post(`${MGMT_SVC}/GetExperiment`, () => {
        return HttpResponse.json(
          { code: 'internal', message: 'Database connection lost' },
          { status: 500 },
        );
      }),
    );

    render(<AuthProvider initialUser={defaultUser}><ExperimentDetailPage /></AuthProvider>);

    await waitFor(() => {
      expect(screen.getByTestId('retryable-error')).toBeInTheDocument();
    });

    expect(screen.getByText(/Database connection lost/)).toBeInTheDocument();
    expect(screen.getByTestId('retry-button')).toBeInTheDocument();
  });
});

describe('Chaos: 404 vs 500 on analysis tabs', () => {
  beforeEach(() => {
    mockExperimentId = '11111111-1111-1111-1111-111111111111';
  });

  it('shows gray "no data" for 404 (data does not exist)', async () => {
    const user = userEvent.setup();

    // Interleaving returns 404 for experiment 11111111 (no seed data)
    render(<ResultsPage />);

    await waitFor(() => {
      expect(screen.getByRole('tab', { name: 'Interleaving' })).toBeInTheDocument();
    });

    await user.click(screen.getByRole('tab', { name: 'Interleaving' }));

    await waitFor(() => {
      expect(screen.getByText('No interleaving analysis available for this experiment.')).toBeInTheDocument();
    });

    // Should NOT show retry button for a 404 (no data = expected state)
    expect(screen.queryByTestId('retryable-error')).not.toBeInTheDocument();
  });

  it('shows red retryable error for 500 (service is down)', async () => {
    const user = userEvent.setup();

    // Override novelty endpoint to return 500
    server.use(
      http.post(`${ANALYSIS_SVC}/GetNoveltyAnalysis`, () => {
        return HttpResponse.json(
          { code: 'internal', message: 'Analysis service crashed' },
          { status: 500 },
        );
      }),
    );

    render(<ResultsPage />);

    await waitFor(() => {
      expect(screen.getByRole('tab', { name: 'Novelty Effects' })).toBeInTheDocument();
    });

    await user.click(screen.getByRole('tab', { name: 'Novelty Effects' }));

    await waitFor(() => {
      expect(screen.getByTestId('retryable-error')).toBeInTheDocument();
    });

    expect(screen.getByText(/Analysis service crashed/)).toBeInTheDocument();
    expect(screen.getByTestId('retry-button')).toBeInTheDocument();
  });
});

describe('Chaos: Backend goes down mid-session', () => {
  beforeEach(() => {
    mockExperimentId = '11111111-1111-1111-1111-111111111111';
  });

  it('shows error on newly-loaded tab after backend fails, recovers on retry', async () => {
    const user = userEvent.setup();

    // Load results page normally first
    render(<ResultsPage />);

    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'Results Dashboard' })).toBeInTheDocument();
    });

    // Now the backend goes down
    server.use(
      http.post(`${ANALYSIS_SVC}/GetNoveltyAnalysis`, () => {
        return HttpResponse.json(
          { code: 'internal', message: 'Backend unreachable' },
          { status: 500 },
        );
      }),
    );
    clearApiCache();

    // Switch to Novelty tab — this triggers a new fetch that hits the 500
    await user.click(screen.getByRole('tab', { name: 'Novelty Effects' }));

    await waitFor(() => {
      expect(screen.getByTestId('retryable-error')).toBeInTheDocument();
    });

    expect(screen.getByText(/Backend unreachable/)).toBeInTheDocument();

    // Restore backend
    server.resetHandlers();
    clearApiCache();

    // Click retry
    await user.click(screen.getByTestId('retry-button'));

    // Tab should now show the actual novelty data
    await waitFor(() => {
      expect(screen.getByText('Novelty Effect Detected')).toBeInTheDocument();
    });

    expect(screen.queryByTestId('retryable-error')).not.toBeInTheDocument();
  });
});

describe('Chaos: Partial outage — page loads but individual tab fails', () => {
  beforeEach(() => {
    mockExperimentId = '11111111-1111-1111-1111-111111111111';
  });

  it('overview tab works while novelty tab shows error', async () => {
    const user = userEvent.setup();

    // Only novelty is down
    server.use(
      http.post(`${ANALYSIS_SVC}/GetNoveltyAnalysis`, () => {
        return HttpResponse.json(
          { code: 'internal', message: 'Partial outage' },
          { status: 500 },
        );
      }),
    );

    render(<ResultsPage />);

    // Overview loads fine
    await waitFor(() => {
      expect(screen.getByText('Metric Results')).toBeInTheDocument();
    });

    // Switch to novelty — it's broken
    await user.click(screen.getByRole('tab', { name: 'Novelty Effects' }));

    await waitFor(() => {
      expect(screen.getByTestId('retryable-error')).toBeInTheDocument();
    });

    expect(screen.getByText(/Partial outage/)).toBeInTheDocument();

    // Switch back to overview — still works fine
    await user.click(screen.getByRole('tab', { name: 'Overview' }));

    await waitFor(() => {
      expect(screen.getByText('Metric Results')).toBeInTheDocument();
    });

    expect(screen.queryByTestId('retryable-error')).not.toBeInTheDocument();
  });
});

describe('Chaos: Network error on list page', () => {
  it('shows retryable error on network failure (fetch TypeError)', async () => {
    server.use(
      http.post(`${MGMT_SVC}/ListExperiments`, () => {
        return HttpResponse.error();
      }),
    );

    render(<AuthProvider initialUser={defaultUser}><ExperimentListPage /></AuthProvider>);

    await waitFor(() => {
      expect(screen.getByTestId('retryable-error')).toBeInTheDocument();
    });

    expect(screen.getByTestId('retry-button')).toBeInTheDocument();
  });
});
