import { render, screen, waitFor } from '@testing-library/react';
import { http, HttpResponse } from 'msw';
import { server } from '@/__mocks__/server';
import { describe, it, expect, vi } from 'vitest';
import MetricBrowserPage from '@/app/metrics/page';
import { ToastProvider } from '@/lib/toast-context';
import { AuthProvider } from '@/lib/auth-context';

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

describe('Metric Browser Page Permission Gating', () => {
  it('shows disabled New Metric button for viewer', async () => {
    render(
      <AuthProvider initialUser={{ email: 'viewer@streamco.com', role: 'viewer' }}>
        <ToastProvider>
          <MetricBrowserPage />
        </ToastProvider>
      </AuthProvider>
    );
    await waitFor(() => {
      expect(screen.getByText('Stream Start Rate')).toBeInTheDocument();
    });

    const button = screen.getByTestId('new-metric-disabled');
    expect(button).toBeInTheDocument();
    expect(button).toHaveAttribute('title', 'Requires Experimenter role (you are Viewer)');
  });

  it('shows enabled New Metric button for experimenter', async () => {
    render(
      <AuthProvider initialUser={{ email: 'exp@streamco.com', role: 'experimenter' }}>
        <ToastProvider>
          <MetricBrowserPage />
        </ToastProvider>
      </AuthProvider>
    );
    await waitFor(() => {
      expect(screen.getByText('Stream Start Rate')).toBeInTheDocument();
    });

    expect(screen.getByTestId('new-metric-button')).toBeInTheDocument();
  });

  it('shows CTA in empty state for experimenter', async () => {
    server.use(
      http.post(`${MGMT_SVC}/ListMetricDefinitions`, () => {
        return HttpResponse.json({ metrics: [], nextPageToken: '' });
      }),
    );

    render(
      <AuthProvider initialUser={{ email: 'exp@streamco.com', role: 'experimenter' }}>
        <ToastProvider>
          <MetricBrowserPage />
        </ToastProvider>
      </AuthProvider>
    );
    await waitFor(() => {
      expect(screen.getByTestId('empty-state')).toBeInTheDocument();
    });

    expect(screen.getByTestId('create-first-metric')).toBeInTheDocument();
  });

  it('does NOT show CTA in empty state for viewer', async () => {
    server.use(
      http.post(`${MGMT_SVC}/ListMetricDefinitions`, () => {
        return HttpResponse.json({ metrics: [], nextPageToken: '' });
      }),
    );

    render(
      <AuthProvider initialUser={{ email: 'viewer@streamco.com', role: 'viewer' }}>
        <ToastProvider>
          <MetricBrowserPage />
        </ToastProvider>
      </AuthProvider>
    );
    await waitFor(() => {
      expect(screen.getByTestId('empty-state')).toBeInTheDocument();
    });

    expect(screen.queryByTestId('create-first-metric')).not.toBeInTheDocument();
  });
});
