import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi } from 'vitest';
import { ExperimentSelector } from '@/components/experiment-selector';
import { ExperimentFiltersToolbar } from '@/components/experiment-filters';
import { AuditFilters } from '@/components/audit-filters';
import { AuthProvider } from '@/lib/auth-context';
import { ToastProvider } from '@/lib/toast-context';
import FlagListPage from '@/app/flags/page';
import MetricBrowserPage from '@/app/metrics/page';
import type { Experiment } from '@/lib/types';

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

// Mock next/dynamic
vi.mock('next/dynamic', () => ({
  default: (loader: () => Promise<{ default: React.ComponentType<unknown> }>) => {
    let Comp: React.ComponentType<unknown> | null = null;
    loader().then((mod) => { Comp = mod.default; });
    return function DynamicMock(props: Record<string, unknown>) {
      return Comp ? <Comp {...props} /> : null;
    };
  },
}));

const mockExperiments: Experiment[] = [
  {
    experimentId: 'exp-1',
    name: 'Running Exp 1',
    description: 'First mock experiment',
    ownerEmail: 'owner1@streamco.com',
    type: 'AB',
    state: 'RUNNING',
    variants: [],
    layerId: 'layer-1',
    hashSalt: 'salt',
    primaryMetricId: 'metric-1',
    secondaryMetricIds: [],
    guardrailConfigs: [],
    guardrailAction: 'ALERT_ONLY',
    isCumulativeHoldout: false,
    createdAt: '2026-01-01T00:00:00Z',
  },
];

describe('Search Clear Buttons Focus Restoration', () => {
  it('ExperimentSelector restores focus to search input when clear search is clicked', async () => {
    const user = userEvent.setup();
    render(
      <ExperimentSelector
        experiments={mockExperiments}
        selectedIds={[]}
        onSelect={vi.fn()}
        onRemove={vi.fn()}
      />
    );

    const input = screen.getByRole('textbox', { name: 'Search experiments' });
    await user.type(input, 'test-query');
    expect(input).toHaveValue('test-query');

    const clearButton = screen.getByRole('button', { name: 'Clear search' });
    await user.click(clearButton);

    expect(input).toHaveValue('');
    expect(input).toHaveFocus();
  });

  it('ExperimentFiltersToolbar restores focus to search input when clear search is clicked', async () => {
    const user = userEvent.setup();
    const setQueryMock = vi.fn();
    const mockFilters = {
      query: 'test-filter-query',
      setQuery: setQueryMock,
      stateFilter: '' as const,
      setStateFilter: vi.fn(),
      typeFilter: '' as const,
      setTypeFilter: vi.fn(),
      sortField: 'createdAt' as const,
      sortDir: 'desc' as const,
      toggleSort: vi.fn(),
      clearFilters: vi.fn(),
      applyFilters: (exps: Experiment[]) => exps,
      hasActiveFilters: true,
    };

    render(
      <ExperimentFiltersToolbar
        filters={mockFilters}
        totalCount={1}
        filteredCount={1}
      />
    );

    const input = screen.getByRole('textbox', { name: 'Search experiments' });
    expect(input).toHaveValue('test-filter-query');

    const clearButton = screen.getByRole('button', { name: 'Clear search' });
    await user.click(clearButton);

    expect(setQueryMock).toHaveBeenCalledWith('');
    expect(input).toHaveFocus();
  });

  it('AuditFilters restores focus to experiment search input when experiment search is cleared', async () => {
    const user = userEvent.setup();
    const onExperimentQueryChangeMock = vi.fn();

    render(
      <AuditFilters
        experimentQuery="audit-exp-query"
        onExperimentQueryChange={onExperimentQueryChangeMock}
        actionFilter=""
        onActionFilterChange={vi.fn()}
        actorQuery=""
        onActorQueryChange={vi.fn()}
        totalCount={10}
        filteredCount={5}
        onClear={vi.fn()}
        hasActiveFilters={true}
      />
    );

    const input = screen.getByRole('textbox', { name: 'Search by experiment name' });
    expect(input).toHaveValue('audit-exp-query');

    const clearButton = screen.getByRole('button', { name: 'Clear experiment search' });
    await user.click(clearButton);

    expect(onExperimentQueryChangeMock).toHaveBeenCalledWith('');
    expect(input).toHaveFocus();
  });

  it('AuditFilters restores focus to actor search input when actor search is cleared', async () => {
    const user = userEvent.setup();
    const onActorQueryChangeMock = vi.fn();

    render(
      <AuditFilters
        experimentQuery=""
        onExperimentQueryChange={vi.fn()}
        actionFilter=""
        onActionFilterChange={vi.fn()}
        actorQuery="actor-query"
        onActorQueryChange={onActorQueryChangeMock}
        totalCount={10}
        filteredCount={5}
        onClear={vi.fn()}
        hasActiveFilters={true}
      />
    );

    const input = screen.getByRole('textbox', { name: 'Filter by actor email' });
    expect(input).toHaveValue('actor-query');

    const clearButton = screen.getByRole('button', { name: 'Clear actor search' });
    await user.click(clearButton);

    expect(onActorQueryChangeMock).toHaveBeenCalledWith('');
    expect(input).toHaveFocus();
  });

  it('FlagListPage restores focus to search input when clear search is clicked', async () => {
    const user = userEvent.setup();
    render(
      <AuthProvider initialUser={{ email: 'test@streamco.com', role: 'admin' }}>
        <ToastProvider>
          <FlagListPage />
        </ToastProvider>
      </AuthProvider>
    );

    await waitFor(() => {
      expect(screen.getByPlaceholderText('Search by name, ID, or description...')).toBeInTheDocument();
    });

    const input = screen.getByPlaceholderText('Search by name, ID, or description...');
    await user.type(input, 'flag-query');
    expect(input).toHaveValue('flag-query');

    const clearButton = screen.getByRole('button', { name: 'Clear search' });
    await user.click(clearButton);

    expect(input).toHaveValue('');
    expect(input).toHaveFocus();
  });

  it('FlagListPage restores focus to search input when empty-state Clear filters is clicked', async () => {
    const user = userEvent.setup();
    render(
      <AuthProvider initialUser={{ email: 'test@streamco.com', role: 'admin' }}>
        <ToastProvider>
          <FlagListPage />
        </ToastProvider>
      </AuthProvider>
    );

    await waitFor(() => {
      expect(screen.getByPlaceholderText('Search by name, ID, or description...')).toBeInTheDocument();
    });

    const input = screen.getByPlaceholderText('Search by name, ID, or description...');
    await user.type(input, 'nonexistent-flag-xyz');

    const clearFiltersBtn = await screen.findByTestId('clear-filters-empty');
    await user.click(clearFiltersBtn);

    expect(input).toHaveValue('');
    expect(input).toHaveFocus();
  });

  it('MetricBrowserPage restores focus to search input when clear search is clicked', async () => {
    const user = userEvent.setup();
    render(
      <AuthProvider initialUser={{ email: 'test@streamco.com', role: 'admin' }}>
        <ToastProvider>
          <MetricBrowserPage />
        </ToastProvider>
      </AuthProvider>
    );

    await waitFor(() => {
      expect(screen.getByPlaceholderText('Search by name, ID, or description...')).toBeInTheDocument();
    });

    const input = screen.getByPlaceholderText('Search by name, ID, or description...');
    await user.type(input, 'metric-query');
    expect(input).toHaveValue('metric-query');

    const clearButton = screen.getByRole('button', { name: 'Clear search' });
    await user.click(clearButton);

    expect(input).toHaveValue('');
    expect(input).toHaveFocus();
  });

  it('MetricBrowserPage restores focus to search input when empty-state Clear filters is clicked', async () => {
    const user = userEvent.setup();
    render(
      <AuthProvider initialUser={{ email: 'test@streamco.com', role: 'admin' }}>
        <ToastProvider>
          <MetricBrowserPage />
        </ToastProvider>
      </AuthProvider>
    );

    await waitFor(() => {
      expect(screen.getByPlaceholderText('Search by name, ID, or description...')).toBeInTheDocument();
    });

    const input = screen.getByPlaceholderText('Search by name, ID, or description...');
    await user.type(input, 'nonexistent-metric-xyz');

    const clearFiltersBtn = await screen.findByTestId('clear-filters-empty');
    await user.click(clearFiltersBtn);

    expect(input).toHaveValue('');
    expect(input).toHaveFocus();
  });
});
