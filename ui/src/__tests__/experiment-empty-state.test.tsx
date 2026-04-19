import { render, screen, waitFor } from '@testing-library/react';
import { describe, it, expect, vi } from 'vitest';
import { http, HttpResponse } from 'msw';
import { server } from '@/__mocks__/server';
import ExperimentListPage from '@/app/page';
import { AuthProvider } from '@/lib/auth-context';
import type { AuthUser } from '@/lib/auth-context';

const MGMT_SVC = '*/experimentation.management.v1.ExperimentManagementService';

// Mock next/navigation
vi.mock('next/navigation', () => ({
  useParams: () => ({}),
  useRouter: () => ({ push: vi.fn(), replace: vi.fn() }),
  useSearchParams: () => new URLSearchParams(),
}));

// Mock next/link
vi.mock('next/link', () => ({
  default: ({ children, href, ...props }: { children: React.ReactNode; href: string; [key: string]: unknown }) => (
    <a href={href} {...props}>{children}</a>
  ),
}));

describe('Experiment List Empty State', () => {
  it('renders empty state with CTA for experimenter', async () => {
    server.use(
      http.post(`${MGMT_SVC}/ListExperiments`, () => {
        return HttpResponse.json({ experiments: [], nextPageToken: '' });
      }),
    );

    const experimenter: AuthUser = { email: 'exp@streamco.com', role: 'experimenter' };
    render(
      <AuthProvider initialUser={experimenter}>
        <ExperimentListPage />
      </AuthProvider>,
    );

    await waitFor(() => {
      expect(screen.getByTestId('empty-state')).toBeInTheDocument();
    });

    expect(screen.getByText('No experiments yet.')).toBeInTheDocument();
    expect(screen.getByTestId('create-first-experiment')).toBeInTheDocument();
    expect(screen.getByTestId('create-first-experiment')).toHaveAttribute('href', '/experiments/new');
  });

  it('renders empty state without CTA for viewer', async () => {
    server.use(
      http.post(`${MGMT_SVC}/ListExperiments`, () => {
        return HttpResponse.json({ experiments: [], nextPageToken: '' });
      }),
    );

    const viewer: AuthUser = { email: 'viewer@streamco.com', role: 'viewer' };
    render(
      <AuthProvider initialUser={viewer}>
        <ExperimentListPage />
      </AuthProvider>,
    );

    await waitFor(() => {
      expect(screen.getByTestId('empty-state')).toBeInTheDocument();
    });

    expect(screen.getByText('No experiments yet.')).toBeInTheDocument();
    expect(screen.queryByTestId('create-first-experiment')).not.toBeInTheDocument();
  });

  it('renders page heading even when empty', async () => {
    server.use(
      http.post(`${MGMT_SVC}/ListExperiments`, () => {
        return HttpResponse.json({ experiments: [], nextPageToken: '' });
      }),
    );

    render(
      <AuthProvider initialUser={{ email: 'test@streamco.com', role: 'viewer' }}>
        <ExperimentListPage />
      </AuthProvider>,
    );

    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'Experiments', level: 1 })).toBeInTheDocument();
    });
  });
});
