import { render, screen, waitFor } from '@testing-library/react';
import { describe, it, expect } from 'vitest';
import ExperimentListPage from '@/app/page';

// Mock next/navigation
vi.mock('next/navigation', () => ({
  useParams: () => ({}),
  useRouter: () => ({ push: vi.fn() }),
}));

// Mock next/link to render an anchor tag
vi.mock('next/link', () => ({
  default: ({ children, href, ...props }: { children: React.ReactNode; href: string; [key: string]: unknown }) => (
    <a href={href} {...props}>{children}</a>
  ),
}));

describe('Experiment List Page', () => {
  it('renders all seed experiments', async () => {
    render(<ExperimentListPage />);

    await waitFor(() => {
      expect(screen.getByText('homepage_recs_v2')).toBeInTheDocument();
    });

    expect(screen.getByText('adaptive_bitrate_v3')).toBeInTheDocument();
    expect(screen.getByText('search_ranking_interleave')).toBeInTheDocument();
    expect(screen.getByText('cold_start_bandit')).toBeInTheDocument();
  });

  it('shows correct state badges', async () => {
    render(<ExperimentListPage />);

    await waitFor(() => {
      expect(screen.getByText('homepage_recs_v2')).toBeInTheDocument();
    });

    // 2 RUNNING + 1 STARTING = some Running badges, 2 DRAFT, 1 STARTING, 1 CONCLUDING
    expect(screen.getAllByText('Running').length).toBeGreaterThanOrEqual(2);
    expect(screen.getAllByText('Draft').length).toBeGreaterThanOrEqual(2);
  });

  it('shows correct type badges', async () => {
    render(<ExperimentListPage />);

    await waitFor(() => {
      expect(screen.getByText('homepage_recs_v2')).toBeInTheDocument();
    });

    expect(screen.getByText('Playback QoE')).toBeInTheDocument();
    expect(screen.getByText('Interleaving')).toBeInTheDocument();
    expect(screen.getByText('Contextual Bandit')).toBeInTheDocument();
  });

  it('displays owner emails', async () => {
    render(<ExperimentListPage />);

    await waitFor(() => {
      expect(screen.getAllByText('alice@streamco.com').length).toBeGreaterThanOrEqual(1);
    });

    expect(screen.getByText('bob@streamco.com')).toBeInTheDocument();
    expect(screen.getByText('carol@streamco.com')).toBeInTheDocument();
    expect(screen.getByText('dave@streamco.com')).toBeInTheDocument();
  });

  it('links experiment names to detail pages', async () => {
    render(<ExperimentListPage />);

    await waitFor(() => {
      expect(screen.getByText('homepage_recs_v2')).toBeInTheDocument();
    });

    const link = screen.getByText('homepage_recs_v2').closest('a');
    expect(link).toHaveAttribute(
      'href',
      '/experiments/11111111-1111-1111-1111-111111111111',
    );
  });
});
