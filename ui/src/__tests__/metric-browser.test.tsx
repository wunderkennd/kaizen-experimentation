import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { http, HttpResponse } from 'msw';
import { server } from '@/__mocks__/server';
import { describe, it, expect } from 'vitest';
import MetricBrowserPage from '@/app/metrics/page';

const MGMT_SVC = '*/experimentation.management.v1.ExperimentManagementService';

// Mock next/navigation
vi.mock('next/navigation', () => ({
  useParams: () => ({}),
  useRouter: () => ({ push: vi.fn(), replace: vi.fn() }),
  useSearchParams: () => new URLSearchParams(),
  usePathname: () => '/metrics',
}));

// Mock next/link to render an anchor tag
vi.mock('next/link', () => ({
  default: ({ children, href, ...props }: { children: React.ReactNode; href: string; [key: string]: unknown }) => (
    <a href={href} {...props}>{children}</a>
  ),
}));

async function renderAndWait() {
  render(<MetricBrowserPage />);
  await waitFor(() => {
    expect(screen.getByText('Stream Start Rate')).toBeInTheDocument();
  });
}

describe('Metric Browser Page', () => {
  it('renders metric list with all seed metrics', async () => {
    await renderAndWait();

    expect(screen.getByText('Stream Start Rate')).toBeInTheDocument();
    expect(screen.getByText('Watch Time (minutes)')).toBeInTheDocument();
    expect(screen.getByText('Content Completion Rate')).toBeInTheDocument();
    expect(screen.getByText('Rebuffer Rate')).toBeInTheDocument();
    expect(screen.getByText('Search Success Rate')).toBeInTheDocument();
    expect(screen.getByText('Recommendation CTR')).toBeInTheDocument();
    expect(screen.getByText('Revenue per User')).toBeInTheDocument();
    expect(screen.getByText('Churn (7-day)')).toBeInTheDocument();
    expect(screen.getByText('Playback Start Latency p50')).toBeInTheDocument();
    expect(screen.getByText('Error Rate')).toBeInTheDocument();
    expect(screen.getByText('Daily Active Users')).toBeInTheDocument();
    expect(screen.getByText('Engagement Score')).toBeInTheDocument();

    // Count badge shows 12
    expect(screen.getByTestId('metric-count')).toHaveTextContent('12');
  });

  it('shows loading spinner initially', () => {
    render(<MetricBrowserPage />);
    expect(screen.getByRole('status', { name: 'Loading' })).toBeInTheDocument();
  });

  it('shows RetryableError + retry on 500', async () => {
    server.use(
      http.post(`${MGMT_SVC}/ListMetricDefinitions`, () => {
        return HttpResponse.json({ message: 'Internal server error' }, { status: 500 });
      }),
    );

    render(<MetricBrowserPage />);
    await waitFor(() => {
      expect(screen.getByTestId('retryable-error')).toBeInTheDocument();
    });

    expect(screen.getByText(/Failed to load metric definitions/)).toBeInTheDocument();

    // Reset to working handler and retry
    server.resetHandlers();
    await userEvent.click(screen.getByTestId('retry-button'));
    await waitFor(() => {
      expect(screen.getByText('Stream Start Rate')).toBeInTheDocument();
    });
  });

  it('shows empty state when no metrics', async () => {
    server.use(
      http.post(`${MGMT_SVC}/ListMetricDefinitions`, () => {
        return HttpResponse.json({ metrics: [], nextPageToken: '' });
      }),
    );

    render(<MetricBrowserPage />);
    await waitFor(() => {
      expect(screen.getByTestId('empty-state')).toBeInTheDocument();
    });
    expect(screen.getByText('No metric definitions found.')).toBeInTheDocument();
  });

  it('filters by type dropdown', async () => {
    await renderAndWait();
    const user = userEvent.setup();

    const typeFilter = screen.getByTestId('type-filter');
    await user.selectOptions(typeFilter, 'PROPORTION');

    // Only PROPORTION metrics should be visible (6 total)
    expect(screen.getByText('Stream Start Rate')).toBeInTheDocument();
    expect(screen.getByText('Content Completion Rate')).toBeInTheDocument();
    expect(screen.getByText('Search Success Rate')).toBeInTheDocument();
    expect(screen.getByText('Churn (7-day)')).toBeInTheDocument();
    expect(screen.getByText('Error Rate')).toBeInTheDocument();
    expect(screen.queryByText('Watch Time (minutes)')).not.toBeInTheDocument();
    expect(screen.queryByText('Rebuffer Rate')).not.toBeInTheDocument();

    // Count badge updates
    expect(screen.getByTestId('metric-count')).toHaveTextContent('6');
  });

  it('searches by name text', async () => {
    await renderAndWait();
    const user = userEvent.setup();

    const searchInput = screen.getByTestId('metric-search');
    await user.type(searchInput, 'Rebuffer');

    expect(screen.getByText('Rebuffer Rate')).toBeInTheDocument();
    expect(screen.queryByText('Stream Start Rate')).not.toBeInTheDocument();
    expect(screen.getByTestId('metric-count')).toHaveTextContent('1');
  });

  it('searches by metric ID text', async () => {
    await renderAndWait();
    const user = userEvent.setup();

    const searchInput = screen.getByTestId('metric-search');
    await user.type(searchInput, 'ctr_recommendation');

    expect(screen.getByText('Recommendation CTR')).toBeInTheDocument();
    expect(screen.queryByText('Stream Start Rate')).not.toBeInTheDocument();
  });

  it('renders correct type badge colors', async () => {
    await renderAndWait();

    const meanBadge = screen.getByTestId('type-badge-watch_time_minutes');
    expect(meanBadge).toHaveTextContent('MEAN');
    expect(meanBadge.className).toContain('bg-blue-100');

    const proportionBadge = screen.getByTestId('type-badge-stream_start_rate');
    expect(proportionBadge).toHaveTextContent('PROPORTION');
    expect(proportionBadge.className).toContain('bg-green-100');

    const ratioBadge = screen.getByTestId('type-badge-rebuffer_rate');
    expect(ratioBadge).toHaveTextContent('RATIO');
    expect(ratioBadge.className).toContain('bg-purple-100');

    const percentileBadge = screen.getByTestId('type-badge-latency_p50_ms');
    expect(percentileBadge).toHaveTextContent('PERCENTILE');
    expect(percentileBadge.className).toContain('bg-amber-100');

    const countBadge = screen.getByTestId('type-badge-daily_active_users');
    expect(countBadge).toHaveTextContent('COUNT');
    expect(countBadge.className).toContain('bg-gray-100');

    const customBadge = screen.getByTestId('type-badge-engagement_score');
    expect(customBadge).toHaveTextContent('CUSTOM');
    expect(customBadge.className).toContain('bg-orange-100');
  });

  it('shows QoE badge on QoE metrics', async () => {
    await renderAndWait();

    // QoE metrics: rebuffer_rate, latency_p50_ms, error_rate
    expect(screen.getByTestId('qoe-badge-rebuffer_rate')).toHaveTextContent('QoE');
    expect(screen.getByTestId('qoe-badge-latency_p50_ms')).toHaveTextContent('QoE');
    expect(screen.getByTestId('qoe-badge-error_rate')).toHaveTextContent('QoE');

    // Non-QoE metrics should NOT have QoE badge
    expect(screen.queryByTestId('qoe-badge-stream_start_rate')).not.toBeInTheDocument();
  });

  it('shows direction indicator for lower-is-better metrics', async () => {
    await renderAndWait();

    // lower-is-better: rebuffer_rate, churn_7d, latency_p50_ms, error_rate
    expect(screen.getByTestId('direction-rebuffer_rate')).toHaveTextContent('↓ lower is better');
    expect(screen.getByTestId('direction-churn_7d')).toHaveTextContent('↓ lower is better');
    expect(screen.getByTestId('direction-latency_p50_ms')).toHaveTextContent('↓ lower is better');

    // higher-is-better
    expect(screen.getByTestId('direction-stream_start_rate')).toHaveTextContent('↑ higher is better');
    expect(screen.getByTestId('direction-watch_time_minutes')).toHaveTextContent('↑ higher is better');
  });

});
