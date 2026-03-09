import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { http, HttpResponse } from 'msw';
import { server } from '@/__mocks__/server';
import SqlPage from '@/app/experiments/[id]/sql/page';

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

describe('SQL Page', () => {
  beforeEach(() => {
    mockExperimentId = '11111111-1111-1111-1111-111111111111';
  });

  it('shows loading state initially', () => {
    render(<SqlPage />);
    expect(document.querySelector('.animate-spin')).toBeInTheDocument();
  });

  it('renders query log entries after load', async () => {
    render(<SqlPage />);

    await waitFor(() => {
      expect(screen.getByText('click_through_rate')).toBeInTheDocument();
    });

    expect(screen.getByText('watch_time_per_session')).toBeInTheDocument();
    expect(screen.getByText('crash_rate')).toBeInTheDocument();
  });

  it('shows metric ID for each entry', async () => {
    render(<SqlPage />);

    await waitFor(() => {
      expect(screen.getByText('click_through_rate')).toBeInTheDocument();
    });

    expect(screen.getByText('watch_time_per_session')).toBeInTheDocument();
    expect(screen.getByText('crash_rate')).toBeInTheDocument();
  });

  it('shows formatted duration and row count', async () => {
    render(<SqlPage />);

    await waitFor(() => {
      expect(screen.getByText('3.2s')).toBeInTheDocument();
    });

    expect(screen.getByText('125,000')).toBeInTheDocument();
    expect(screen.getByText('4.1s')).toBeInTheDocument();
    expect(screen.getByText('1.8s')).toBeInTheDocument();
  });

  it('expands row to show full SQL text', async () => {
    const user = userEvent.setup();
    render(<SqlPage />);

    await waitFor(() => {
      expect(screen.getByText('click_through_rate')).toBeInTheDocument();
    });

    // Click the SQL preview button for the first entry
    const sqlPreviews = screen.getAllByRole('button', { name: /SELECT/i });
    await user.click(sqlPreviews[0]);

    // Should show the full SQL in a <pre> block
    const preElements = document.querySelectorAll('pre');
    expect(preElements.length).toBeGreaterThanOrEqual(1);
    expect(preElements[0].textContent).toContain('experiment_id');
  });

  it('shows empty state when no entries', async () => {
    mockExperimentId = '44444444-4444-4444-4444-444444444444';
    render(<SqlPage />);

    await waitFor(() => {
      expect(screen.getByText('No query log entries found for this experiment.')).toBeInTheDocument();
    });
  });

  it('shows Export Notebook button', async () => {
    render(<SqlPage />);

    await waitFor(() => {
      expect(screen.getByText('Export Notebook')).toBeInTheDocument();
    });
  });

  it('renders error state on API failure', async () => {
    server.use(
      http.post('*/experimentation.metrics.v1.MetricComputationService/GetQueryLog', () => {
        return HttpResponse.json(
          { code: 'internal', message: 'Internal server error' },
          { status: 500 },
        );
      }),
    );

    render(<SqlPage />);

    await waitFor(() => {
      expect(screen.getByText(/Internal server error/)).toBeInTheDocument();
    });
  });

  it('renders breadcrumb navigation', async () => {
    render(<SqlPage />);

    await waitFor(() => {
      expect(screen.getByText('Query Log')).toBeInTheDocument();
    });

    const experimentsLink = screen.getAllByText('Experiments')[0];
    expect(experimentsLink.closest('a')).toHaveAttribute('href', '/');

    const detailLink = screen.getAllByText('Detail')[0];
    expect(detailLink.closest('a')).toHaveAttribute(
      'href',
      '/experiments/11111111-1111-1111-1111-111111111111',
    );
  });
});
