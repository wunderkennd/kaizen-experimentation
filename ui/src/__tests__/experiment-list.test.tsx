import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect } from 'vitest';
import ExperimentListPage from '@/app/page';
import { AuthProvider } from '@/lib/auth-context';
import type { AuthUser } from '@/lib/auth-context';

const defaultUser: AuthUser = { email: 'test@streamco.com', role: 'experimenter' };

// Mock next/navigation
vi.mock('next/navigation', () => ({
  useParams: () => ({}),
  useRouter: () => ({ push: vi.fn(), replace: vi.fn() }),
  useSearchParams: () => new URLSearchParams(),
}));

// Mock next/link to render an anchor tag
vi.mock('next/link', () => ({
  default: ({ children, href, ...props }: { children: React.ReactNode; href: string; [key: string]: unknown }) => (
    <a href={href} {...props}>{children}</a>
  ),
}));

async function renderAndWait(user: AuthUser = defaultUser) {
  render(
    <AuthProvider initialUser={user}>
      <ExperimentListPage />
    </AuthProvider>,
  );
  await waitFor(() => {
    expect(screen.getByText('homepage_recs_v2')).toBeInTheDocument();
  });
}

describe('Experiment List Page', () => {
  it('renders all seed experiments including CONCLUDED and ARCHIVED', async () => {
    await renderAndWait();

    expect(screen.getByText('homepage_recs_v2')).toBeInTheDocument();
    expect(screen.getByText('adaptive_bitrate_v3')).toBeInTheDocument();
    expect(screen.getByText('search_ranking_interleave')).toBeInTheDocument();
    expect(screen.getByText('cold_start_bandit')).toBeInTheDocument();
    expect(screen.getByText('onboarding_flow_v2')).toBeInTheDocument();
    expect(screen.getByText('thumbnail_selection_v1')).toBeInTheDocument();
    expect(screen.getByText('recommendation_holdout_q1')).toBeInTheDocument();
    expect(screen.getByText('retention_nudge_v1')).toBeInTheDocument();
    expect(screen.getByText('session_watch_pattern')).toBeInTheDocument();
    expect(screen.getByText('legacy_layout_test')).toBeInTheDocument();
  });

  it('shows correct state badges', async () => {
    await renderAndWait();

    // State labels appear in both filter dropdown and table badges.
    // Table badges: 5 RUNNING (incl META), 2 DRAFT, 1 STARTING, 1 CONCLUDING, 4 CONCLUDED, 1 ARCHIVED
    // Dropdown options add 1 of each. So Running = 5+1=6, Draft = 2+1=3, etc.
    expect(screen.getAllByText('Running').length).toBe(6);
    expect(screen.getAllByText('Draft').length).toBe(3);
    expect(screen.getAllByText('Starting').length).toBe(2);
    expect(screen.getAllByText('Concluding').length).toBe(2);
    expect(screen.getAllByText('Concluded').length).toBe(5);
    expect(screen.getAllByText('Archived').length).toBe(2);
  });

  it('shows correct type badges', async () => {
    await renderAndWait();

    // Type labels appear in both filter dropdown and table badges.
    // Each type in the table also appears once in the dropdown.
    expect(screen.getAllByText('Playback QoE').length).toBe(2); // 1 badge + 1 dropdown
    expect(screen.getAllByText('Interleaving').length).toBe(2);
    expect(screen.getAllByText('Contextual Bandit').length).toBe(2);
    expect(screen.getAllByText('Multivariate').length).toBe(2);
    expect(screen.getAllByText('Cumulative Holdout').length).toBe(2);
  });

  it('displays owner emails', async () => {
    await renderAndWait();

    expect(screen.getAllByText('alice@streamco.com').length).toBeGreaterThanOrEqual(2);
    expect(screen.getAllByText('bob@streamco.com').length).toBeGreaterThanOrEqual(2);
    expect(screen.getAllByText('carol@streamco.com').length).toBeGreaterThanOrEqual(1);
    expect(screen.getByText('dave@streamco.com')).toBeInTheDocument();
  });

  it('links experiment names to detail pages', async () => {
    await renderAndWait();

    const link = screen.getByText('homepage_recs_v2').closest('a');
    expect(link).toHaveAttribute(
      'href',
      '/experiments/11111111-1111-1111-1111-111111111111',
    );
  });

  // --- Search tests ---

  it('renders search input', async () => {
    await renderAndWait();

    expect(screen.getByPlaceholderText('Search experiments...')).toBeInTheDocument();
  });

  it('filters by text query', async () => {
    const user = userEvent.setup();
    await renderAndWait();

    const searchInput = screen.getByPlaceholderText('Search experiments...');
    await user.type(searchInput, 'homepage');

    await waitFor(() => {
      expect(screen.getByText('homepage_recs_v2')).toBeInTheDocument();
      expect(screen.queryByText('adaptive_bitrate_v3')).not.toBeInTheDocument();
    });
  });

  it('state dropdown filters to selected state', async () => {
    const user = userEvent.setup();
    await renderAndWait();

    const stateSelect = screen.getByLabelText('Filter by state');
    await user.selectOptions(stateSelect, 'DRAFT');

    await waitFor(() => {
      expect(screen.getByText('adaptive_bitrate_v3')).toBeInTheDocument();
      expect(screen.getByText('cold_start_bandit')).toBeInTheDocument();
      expect(screen.queryByText('homepage_recs_v2')).not.toBeInTheDocument();
    });
  });

  it('type dropdown filters to selected type', async () => {
    const user = userEvent.setup();
    await renderAndWait();

    const typeSelect = screen.getByLabelText('Filter by type');
    await user.selectOptions(typeSelect, 'INTERLEAVING');

    await waitFor(() => {
      expect(screen.getByText('search_ranking_interleave')).toBeInTheDocument();
      expect(screen.queryByText('homepage_recs_v2')).not.toBeInTheDocument();
    });
  });

  it('combined filters narrow results', async () => {
    const user = userEvent.setup();
    await renderAndWait();

    const stateSelect = screen.getByLabelText('Filter by state');
    await user.selectOptions(stateSelect, 'RUNNING');

    await waitFor(() => {
      // 3 RUNNING experiments
      expect(screen.getByText('homepage_recs_v2')).toBeInTheDocument();
      expect(screen.getByText('search_ranking_interleave')).toBeInTheDocument();
      expect(screen.getByText('recommendation_holdout_q1')).toBeInTheDocument();
    });

    const typeSelect = screen.getByLabelText('Filter by type');
    await user.selectOptions(typeSelect, 'AB');

    await waitFor(() => {
      expect(screen.getByText('homepage_recs_v2')).toBeInTheDocument();
      expect(screen.queryByText('search_ranking_interleave')).not.toBeInTheDocument();
    });
  });

  it('clear filters button resets all filters', async () => {
    const user = userEvent.setup();
    await renderAndWait();

    const stateSelect = screen.getByLabelText('Filter by state');
    await user.selectOptions(stateSelect, 'DRAFT');

    await waitFor(() => {
      expect(screen.queryByText('homepage_recs_v2')).not.toBeInTheDocument();
    });

    const clearBtn = screen.getByText('Clear filters');
    await user.click(clearBtn);

    await waitFor(() => {
      expect(screen.getByText('homepage_recs_v2')).toBeInTheDocument();
    });
  });

  it('shows "Showing X of Y" count', async () => {
    await renderAndWait();

    const count = screen.getByTestId('filter-count');
    expect(count).toHaveTextContent('Showing 14 of 14 experiments');
  });

  it('no-match empty state displays with clear button', async () => {
    const user = userEvent.setup();
    await renderAndWait();

    const searchInput = screen.getByPlaceholderText('Search experiments...');
    await user.type(searchInput, 'nonexistent_experiment_xyz');

    await waitFor(() => {
      expect(screen.getByTestId('no-filter-matches')).toBeInTheDocument();
      expect(screen.getByText('No experiments match your filters.')).toBeInTheDocument();
    });
  });

  // --- Sort tests ---

  it('sortable header click changes sort order', async () => {
    const user = userEvent.setup();
    await renderAndWait();

    // Click Name header button to sort by name ascending
    const nameHeader = screen.getByRole('button', { name: /Name/ });
    await user.click(nameHeader);

    await waitFor(() => {
      // Verify the sort is ascending by checking aria-sort attribute changed
      const nameHeader = screen.getByRole('button', { name: /Name/ }).closest('th');
      expect(nameHeader?.getAttribute('aria-sort')).toBe('ascending');
    });
    // Verify alphabetically-first experiment is visible
    expect(screen.getByText('adaptive_bitrate_v3')).toBeInTheDocument();
  });

  it('default sort is by created date descending', async () => {
    await renderAndWait();

    const rows = screen.getAllByRole('row');
    // Most recent createdAt is homepage_slate_v1 (2026-03-20), then onboarding_flow_v2 (2026-03-03)
    // So descending: homepage_slate_v1 should be first
    const firstDataRow = rows[1];
    expect(within(firstDataRow).getByText('homepage_slate_v1')).toBeInTheDocument();
  });

  // --- Results link tests ---

  it('CONCLUDED experiment shows "Results available" link', async () => {
    await renderAndWait();

    const resultsLinks = screen.getAllByText('Results available');
    expect(resultsLinks.length).toBeGreaterThanOrEqual(1);
    const link = resultsLinks[0].closest('a');
    expect(link).toHaveAttribute('href', expect.stringContaining('/results'));
  });

  it('RUNNING experiment shows "Interim results" link', async () => {
    await renderAndWait();

    const interimLinks = screen.getAllByText('Interim results');
    expect(interimLinks.length).toBeGreaterThanOrEqual(1);
    // homepage_recs_v2 is RUNNING
    const homepageRow = screen.getByText('homepage_recs_v2').closest('tr')!;
    expect(within(homepageRow).getByText('Interim results')).toBeInTheDocument();
  });

  it('ARCHIVED experiment shows correct state badge', async () => {
    await renderAndWait();

    const legacyRow = screen.getByText('legacy_layout_test').closest('tr')!;
    expect(within(legacyRow).getByText('Archived')).toBeInTheDocument();
  });

  it('CONCLUDING experiment shows "Finalizing..." text', async () => {
    await renderAndWait();

    const thumbnailRow = screen.getByText('thumbnail_selection_v1').closest('tr')!;
    expect(within(thumbnailRow).getByText('Finalizing...')).toBeInTheDocument();
  });

  it('DRAFT experiment shows no results link', async () => {
    await renderAndWait();

    const draftRow = screen.getByText('adaptive_bitrate_v3').closest('tr')!;
    expect(within(draftRow).queryByText('Results available')).not.toBeInTheDocument();
    expect(within(draftRow).queryByText('Interim results')).not.toBeInTheDocument();
    expect(within(draftRow).queryByText('Finalizing...')).not.toBeInTheDocument();
  });
});
