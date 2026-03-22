import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { http, HttpResponse } from 'msw';
import { server } from '@/__mocks__/server';
import { describe, it, expect } from 'vitest';
import AuditLogPage from '@/app/audit/page';

const MGMT_SVC = '*/experimentation.management.v1.ExperimentManagementService';

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

async function renderAndWait() {
  render(<AuditLogPage />);
  await waitFor(() => {
    expect(screen.getByRole('heading', { name: 'Audit Log' })).toBeInTheDocument();
    expect(screen.getAllByText('homepage_recs_v2').length).toBeGreaterThanOrEqual(1);
  });
}

describe('Audit Log Page', () => {
  it('renders page heading', async () => {
    await renderAndWait();
    expect(screen.getByRole('heading', { name: 'Audit Log' })).toBeInTheDocument();
  });

  it('shows audit log entries in table', async () => {
    await renderAndWait();

    // Check that entries from seed data are visible
    expect(screen.getAllByText('homepage_recs_v2').length).toBeGreaterThanOrEqual(1);
    expect(screen.getAllByText('search_ranking_boost').length).toBeGreaterThanOrEqual(1);
    expect(screen.getAllByText('playback_buffer_strategy').length).toBeGreaterThanOrEqual(1);

    // Check actors
    expect(screen.getAllByText('alice@streamco.com').length).toBeGreaterThanOrEqual(1);
    expect(screen.getAllByText('bob@streamco.com').length).toBeGreaterThanOrEqual(1);

    // Verify count shows 15
    expect(screen.getByTestId('audit-count')).toHaveTextContent('15 of 15');
  });

  it('action badges have correct colors', async () => {
    await renderAndWait();

    // CREATED badges should have green classes
    const createdBadge = screen.getAllByTestId('action-badge-CREATED')[0];
    expect(createdBadge).toHaveTextContent('Created');
    expect(createdBadge.className).toContain('bg-green-100');

    // GUARDRAIL_BREACH badge should have red classes
    const breachBadge = screen.getByTestId('action-badge-GUARDRAIL_BREACH');
    expect(breachBadge).toHaveTextContent('Guardrail Breach');
    expect(breachBadge.className).toContain('bg-red-100');

    // CONFIG_CHANGED badges should have orange classes
    const configBadge = screen.getAllByTestId('action-badge-CONFIG_CHANGED')[0];
    expect(configBadge).toHaveTextContent('Config Changed');
    expect(configBadge.className).toContain('bg-orange-100');

    // STARTED badges should have blue classes
    const startedBadge = screen.getAllByTestId('action-badge-STARTED')[0];
    expect(startedBadge).toHaveTextContent('Started');
    expect(startedBadge.className).toContain('bg-blue-100');

    // CONCLUDED badges should have indigo classes
    const concludedBadge = screen.getAllByTestId('action-badge-CONCLUDED')[0];
    expect(concludedBadge).toHaveTextContent('Concluded');
    expect(concludedBadge.className).toContain('bg-indigo-100');

    // ARCHIVED badge should have gray classes
    const archivedBadge = screen.getByTestId('action-badge-ARCHIVED');
    expect(archivedBadge).toHaveTextContent('Archived');
    expect(archivedBadge.className).toContain('bg-gray-100');
  });

  it('filter by action type works', async () => {
    const user = userEvent.setup();
    await renderAndWait();

    // Filter by GUARDRAIL_BREACH
    const actionSelect = screen.getByLabelText('Filter by action');
    await user.selectOptions(actionSelect, 'GUARDRAIL_BREACH');

    // Should show only 1 entry
    expect(screen.getByTestId('audit-count')).toHaveTextContent('1 of 15');
    expect(screen.getByTestId('action-badge-GUARDRAIL_BREACH')).toBeInTheDocument();

    // Other action badges should not appear
    expect(screen.queryByTestId('action-badge-CREATED')).not.toBeInTheDocument();
    expect(screen.queryByTestId('action-badge-STARTED')).not.toBeInTheDocument();
  });

  it('filter by experiment name works', async () => {
    const user = userEvent.setup();
    await renderAndWait();

    // Search for search_ranking
    const searchInput = screen.getByLabelText('Search by experiment name');
    await user.type(searchInput, 'search_ranking');

    // Should show only search_ranking_boost entries (audit-004, audit-005, audit-011, audit-012)
    expect(screen.getByTestId('audit-count')).toHaveTextContent('4 of 15');
    expect(screen.queryByText('homepage_recs_v2')).not.toBeInTheDocument();
    expect(screen.getAllByText('search_ranking_boost').length).toBeGreaterThanOrEqual(1);
  });

  it('expanding a row shows details', async () => {
    const user = userEvent.setup();
    await renderAndWait();

    // Click on a CONFIG_CHANGED row (audit-002) which has previousValue/newValue
    const row = screen.getByTestId('audit-row-audit-002');
    await user.click(row);

    // Should show the detail panel
    const detail = screen.getByTestId('audit-detail-audit-002');
    expect(detail).toBeInTheDocument();
    expect(within(detail).getByText(/Previous:/)).toBeInTheDocument();
    expect(within(detail).getByText(/New:/)).toBeInTheDocument();

    // Click again to collapse
    await user.click(row);
    expect(screen.queryByTestId('audit-detail-audit-002')).not.toBeInTheDocument();
  });

  it('pagination loads more entries', async () => {
    // Set up the initial response with a page token, then a second page
    server.use(
      http.post(`${MGMT_SVC}/ListAuditLog`, async ({ request }) => {
        const body = await request.json() as Record<string, unknown>;
        if (body.pageToken === '5') {
          return HttpResponse.json({
            entries: [
              {
                entryId: 'audit-extra-1',
                experimentId: '11111111-1111-1111-1111-111111111111',
                experimentName: 'homepage_recs_v2',
                action: 'UPDATED',
                actorEmail: 'extra@streamco.com',
                timestamp: '2026-03-10T12:00:00Z',
                details: 'Extra entry from page 2',
              },
            ],
            nextPageToken: '',
          });
        }
        return HttpResponse.json({
          entries: [
            {
              entryId: 'audit-page1-1',
              experimentId: '11111111-1111-1111-1111-111111111111',
              experimentName: 'homepage_recs_v2',
              action: 'CREATED',
              actorEmail: 'alice@streamco.com',
              timestamp: '2026-01-15T09:00:00Z',
              details: 'Created experiment',
            },
          ],
          nextPageToken: '5',
        });
      }),
    );

    render(<AuditLogPage />);
    await waitFor(() => {
      expect(screen.getByText('Created experiment')).toBeInTheDocument();
    });

    // Load more button should be visible
    const loadMoreBtn = screen.getByTestId('load-more-button');
    expect(loadMoreBtn).toBeInTheDocument();

    const user = userEvent.setup();
    await user.click(loadMoreBtn);

    await waitFor(() => {
      expect(screen.getByText('Extra entry from page 2')).toBeInTheDocument();
    });

    // Load more button should be gone after last page
    expect(screen.queryByTestId('load-more-button')).not.toBeInTheDocument();
  });

  it('empty state when no entries match filters', async () => {
    const user = userEvent.setup();
    await renderAndWait();

    // Search for something that doesn't exist
    const searchInput = screen.getByLabelText('Search by experiment name');
    await user.type(searchInput, 'nonexistent_experiment_xyz');

    expect(screen.getByTestId('no-filter-matches')).toBeInTheDocument();
    expect(screen.getByText('No audit log entries match your filters.')).toBeInTheDocument();
  });
});
