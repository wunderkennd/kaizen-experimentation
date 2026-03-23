import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { http, HttpResponse } from 'msw';
import { server } from '@/__mocks__/server';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import ProviderHealthPage from '@/app/portfolio/provider-health/page';
import { SEED_PROVIDER_HEALTH } from '@/__mocks__/seed-data';

const METRICS_SVC = '*/experimentation.metrics.v1.MetricComputationService';

// Mock next/navigation
vi.mock('next/navigation', () => ({
  useParams: () => ({}),
  useRouter: () => ({ push: vi.fn(), replace: vi.fn() }),
  useSearchParams: () => new URLSearchParams(),
  usePathname: () => '/portfolio/provider-health',
}));

// Mock next/link
vi.mock('next/link', () => ({
  default: ({ children, href, ...props }: { children: React.ReactNode; href: string; [key: string]: unknown }) => (
    <a href={href} {...props}>{children}</a>
  ),
}));

// Mock dynamic imports so chart components render synchronously in tests
vi.mock('next/dynamic', () => ({
  default: (fn: () => Promise<{ default: React.ComponentType<unknown> }>) => {
    // Return a placeholder that renders a div with data-testid
    const Comp = (props: Record<string, unknown>) => {
      const { series } = props as { series: unknown[] };
      return <div data-testid="chart-component" data-series-count={series?.length ?? 0} />;
    };
    void fn; // suppress unused warning
    return Comp;
  },
}));

async function renderAndWait() {
  render(<ProviderHealthPage />);
  await waitFor(() => {
    expect(screen.getByRole('heading', { name: 'Provider Health', level: 1 })).toBeInTheDocument();
  });
}

describe('Provider Health Page', () => {
  beforeEach(() => {
    // Default handler is already registered in __mocks__/handlers.ts
  });

  it('renders page heading and description', async () => {
    await renderAndWait();
    expect(screen.getByRole('heading', { name: 'Provider Health', level: 1 })).toBeInTheDocument();
    expect(screen.getByText(/catalog coverage, gini concentration/i)).toBeInTheDocument();
  });

  it('renders provider dropdown with all provider options', async () => {
    await renderAndWait();
    const select = screen.getByTestId('provider-filter');
    expect(select).toBeInTheDocument();

    // Should have "All providers" + one option per unique provider
    const uniqueProviders = SEED_PROVIDER_HEALTH.providers;
    const options = Array.from((select as HTMLSelectElement).options);
    expect(options[0].value).toBe('');
    expect(options[0].text).toBe('All providers');
    expect(options.length).toBe(1 + uniqueProviders.length);
  });

  it('renders chart components with series data', async () => {
    await renderAndWait();
    const charts = screen.getAllByTestId('chart-component');
    // 3 charts rendered (coverage, gini, long-tail)
    expect(charts).toHaveLength(3);
    // All series present when no filter
    for (const chart of charts) {
      expect(Number(chart.getAttribute('data-series-count'))).toBeGreaterThan(0);
    }
  });

  it('filters series when a provider is selected', async () => {
    const user = userEvent.setup();
    // Override handler to return filtered data for a specific provider
    server.use(
      http.post(`${METRICS_SVC}/GetProviderHealth`, async ({ request }) => {
        const body = await request.json() as { providerId?: string };
        const filtered = SEED_PROVIDER_HEALTH.series.filter(
          (s) => !body.providerId || s.providerId === body.providerId,
        );
        return HttpResponse.json({ ...SEED_PROVIDER_HEALTH, series: filtered });
      }),
    );

    render(<ProviderHealthPage />);
    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'Provider Health', level: 1 })).toBeInTheDocument();
    });

    const select = screen.getByTestId('provider-filter');
    const targetProvider = SEED_PROVIDER_HEALTH.providers[0];
    await user.selectOptions(select, targetProvider.providerId);

    // After filter change, data should reload
    await waitFor(() => {
      const charts = screen.getAllByTestId('chart-component');
      // Filtered: only series matching prov-originals (2 series)
      const expectedCount = SEED_PROVIDER_HEALTH.series.filter(
        (s) => s.providerId === targetProvider.providerId,
      ).length;
      for (const chart of charts) {
        expect(Number(chart.getAttribute('data-series-count'))).toBe(expectedCount);
      }
    });
  });

  it('shows error state when API fails', async () => {
    server.use(
      http.post(`${METRICS_SVC}/GetProviderHealth`, () => {
        return HttpResponse.json({ message: 'Internal server error' }, { status: 500 });
      }),
    );

    render(<ProviderHealthPage />);
    await waitFor(() => {
      expect(screen.getByRole('button', { name: /retry/i })).toBeInTheDocument();
    });
  });

  it('shows empty state when no series returned', async () => {
    server.use(
      http.post(`${METRICS_SVC}/GetProviderHealth`, () => {
        return HttpResponse.json({
          ...SEED_PROVIDER_HEALTH,
          series: [],
        });
      }),
    );

    render(<ProviderHealthPage />);
    await waitFor(() => {
      expect(screen.getByText(/no data available/i)).toBeInTheDocument();
    });
  });

  it('displays computed-at timestamp', async () => {
    await renderAndWait();
    expect(screen.getByTestId('computed-at')).toBeInTheDocument();
  });

  it('has breadcrumb linking to /portfolio', async () => {
    await renderAndWait();
    const portfolioLink = screen.getByRole('link', { name: 'Portfolio' });
    expect(portfolioLink).toHaveAttribute('href', '/portfolio');
  });
});
