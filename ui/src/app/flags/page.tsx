'use client';

import { useEffect, useState, useCallback, useMemo, useRef } from 'react';
import Link from 'next/link';
import { useSearchShortcut } from '@/hooks/use-search-shortcut';
import type { Flag, FlagType } from '@/lib/types';
import { listFlags } from '@/lib/api';
import { RetryableError } from '@/components/retryable-error';
import { CopyButton } from '@/components/copy-button';
import { useAuth } from '@/lib/auth-context';
import { Breadcrumb } from '@/components/breadcrumb';
import { ROLE_LABELS } from '@/lib/auth';

const FLAG_TYPE_BADGE: Record<FlagType, string> = {
  BOOLEAN: 'bg-blue-100 text-blue-800',
  STRING: 'bg-green-100 text-green-800',
  NUMERIC: 'bg-purple-100 text-purple-800',
  JSON: 'bg-orange-100 text-orange-800',
};

const ALL_FLAG_TYPES: FlagType[] = ['BOOLEAN', 'STRING', 'NUMERIC', 'JSON'];

function FlagListContent() {
  const { canAtLeast, user } = useAuth();
  const [flags, setFlags] = useState<Flag[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [search, setSearch] = useState('');
  const [typeFilter, setTypeFilter] = useState<FlagType | ''>('');
  const inputRef = useRef<HTMLInputElement>(null);
  useSearchShortcut(inputRef);

  const hasActiveFilters = search !== '' || typeFilter !== '';

  const clearFilters = useCallback(() => {
    setSearch('');
    setTypeFilter('');
  }, []);

  const fetchData = useCallback(() => {
    setLoading(true);
    setError(null);
    listFlags()
      .then((data) => {
        setFlags(data.flags);
      })
      .catch((err) => {
        setError(err.message);
      })
      .finally(() => {
        setLoading(false);
      });
  }, []);

  useEffect(() => { fetchData(); }, [fetchData]);

  const filtered = useMemo(() => {
    let result = flags;
    if (typeFilter) {
      result = result.filter((f) => f.type === typeFilter);
    }
    if (search) {
      const q = search.toLowerCase();
      result = result.filter(
        (f) =>
          f.name.toLowerCase().includes(q) ||
          f.description.toLowerCase().includes(q) ||
          f.flagId.toLowerCase().includes(q),
      );
    }
    return result;
  }, [flags, typeFilter, search]);

  return (
    <div>
      <Breadcrumb items={[
        { label: 'Experiments', href: '/' },
        { label: 'Flags' },
      ]} />

      <div className="mb-6 flex items-center justify-between">
        <div className="flex items-center gap-3">
          <h1 className="text-2xl font-bold text-gray-900">Feature Flags</h1>
          {!loading && !error && flags.length > 0 && (
            <span className="inline-flex items-center rounded-full bg-gray-100 px-2.5 py-0.5 text-xs font-medium text-gray-700" data-testid="flag-count">
              {filtered.length}
            </span>
          )}
        </div>
        {canAtLeast('experimenter') ? (
          <Link
            href="/flags/new"
            className="rounded-md bg-indigo-600 px-3 py-2 text-sm font-medium text-white shadow-sm hover:bg-indigo-500 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500 focus-visible:ring-offset-2"
            data-testid="new-flag-button"
          >
            New Flag
          </Link>
        ) : (
          <span
            className="rounded-md bg-indigo-600 px-3 py-2 text-sm font-medium text-white opacity-50 cursor-not-allowed focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500 focus-visible:ring-offset-2"
            title={`Requires Experimenter role (you are ${user ? ROLE_LABELS[user.role] : 'Unknown'})`}
            data-testid="new-flag-disabled"
            tabIndex={0}
            role="button"
            aria-disabled="true"
          >
            New Flag
          </span>
        )}
      </div>

      {loading ? (
        <div className="flex items-center justify-center py-12" role="status" aria-label="Loading">
          <div className="h-8 w-8 animate-spin rounded-full border-4 border-gray-300 border-t-indigo-600" />
          <span className="sr-only">Loading</span>
        </div>
      ) : error ? (
        <RetryableError message={error} onRetry={fetchData} context="feature flags" />
      ) : flags.length === 0 ? (
        <div className="py-12 text-center" data-testid="empty-state">
          <p className="text-sm text-gray-500">No feature flags found.</p>
          {canAtLeast('experimenter') && (
            <div className="mt-6">
              <Link
                href="/flags/new"
                className="rounded-md bg-indigo-600 px-4 py-2 text-sm font-medium text-white hover:bg-indigo-700 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500 focus-visible:ring-offset-2"
                data-testid="create-first-flag"
              >
                Create your first feature flag
              </Link>
            </div>
          )}
        </div>
      ) : (
        <>

      <div className="mb-4 flex flex-wrap items-center gap-3">
        <div className="group relative flex-1 min-w-[300px] max-w-md">
          <svg
            className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-gray-400"
            fill="none"
            stroke="currentColor"
            viewBox="0 0 24 24"
            aria-hidden="true"
          >
            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z" />
          </svg>
          <input
            ref={inputRef}
            type="text"
            placeholder="Search by name, ID, or description..."
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            className="w-full rounded-md border border-gray-300 py-1.5 pl-9 pr-10 text-sm shadow-sm placeholder:text-gray-400 focus:border-indigo-500 focus:outline-none focus:ring-1 focus:ring-indigo-500"
            data-testid="flag-search"
            aria-label="Search flags"
          />
          {search ? (
            <button
              type="button"
              onClick={() => setSearch('')}
              className="absolute right-3 top-1/2 -translate-y-1/2 text-gray-400 hover:text-gray-600 focus:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500 rounded-sm"
              aria-label="Clear search"
              data-testid="clear-search-button"
            >
              <svg className="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" aria-hidden="true">
                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M6 18L18 6M6 6l12 12" />
              </svg>
            </button>
          ) : (
            <div className="pointer-events-none absolute right-3 top-1/2 flex -translate-y-1/2 items-center group-focus-within:hidden group-hover:hidden">
              <span className="flex h-5 w-5 items-center justify-center rounded border border-gray-300 bg-gray-50 text-[10px] font-medium text-gray-500">
                /
              </span>
            </div>
          )}
        </div>
        <select
          value={typeFilter}
          onChange={(e) => setTypeFilter(e.target.value as FlagType | '')}
          className="rounded-md border border-gray-300 px-3 py-1.5 text-sm shadow-sm focus:border-indigo-500 focus:outline-none focus:ring-1 focus:ring-indigo-500"
          data-testid="type-filter"
          aria-label="Filter by flag type"
        >
          <option value="">All Types</option>
          {ALL_FLAG_TYPES.map((t) => (
            <option key={t} value={t}>{t}</option>
          ))}
        </select>
        {hasActiveFilters && (
          <button
            onClick={clearFilters}
            className="rounded-md border border-gray-300 px-3 py-1.5 text-sm text-gray-600 hover:bg-gray-50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500 focus-visible:ring-offset-2"
            data-testid="clear-filters-toolbar"
          >
            Clear filters
          </button>
        )}
      </div>

          {filtered.length === 0 ? (
            <div className="py-12 text-center" data-testid="no-filter-matches">
              <p className="text-sm text-gray-500">No flags match your filters.</p>
              <button
                onClick={clearFilters}
                className="mt-2 rounded-sm text-sm text-indigo-600 hover:text-indigo-800 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500 focus-visible:ring-offset-2"
                data-testid="clear-filters-empty"
              >
                Clear filters
              </button>
            </div>
          ) : (
            <div className="overflow-hidden rounded-lg border border-gray-200 bg-white shadow-sm">
              <table className="min-w-full divide-y divide-gray-200">
                <thead className="bg-gray-50">
                  <tr>
                    <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">Name</th>
                    <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">Flag ID</th>
                    <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">Type</th>
                    <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">Default</th>
                    <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">Enabled</th>
                    <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">Rollout %</th>
                    <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">Variants</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-gray-200">
                  {filtered.map((f) => (
                    <tr key={f.flagId} className="group hover:bg-gray-50 focus-within:bg-gray-50 focus-within:outline-none focus-within:ring-2 focus-within:ring-inset focus-within:ring-indigo-500" data-testid={`flag-row-${f.flagId}`}>
                      <td className="px-4 py-3">
                        <Link href={`/flags/${f.flagId}`} className="font-medium text-indigo-600 hover:text-indigo-800">
                          {f.name}
                        </Link>
                      </td>
                      <td className="px-4 py-3">
                        <div className="flex items-center gap-2">
                          <code className="text-xs text-gray-500">{f.flagId}</code>
                          <CopyButton
                            value={f.flagId}
                            label="Copy flag ID"
                            successMessage="Flag ID copied"
                            className="h-4 w-4 opacity-0 transition-opacity group-hover:opacity-100 group-focus-within:opacity-100"
                          />
                        </div>
                      </td>
                      <td className="px-4 py-3">
                        <span className={`inline-flex rounded-full px-2 py-0.5 text-xs font-medium ${FLAG_TYPE_BADGE[f.type] || 'bg-gray-100 text-gray-800'}`}>
                          {f.type}
                        </span>
                      </td>
                      <td className="px-4 py-3 text-sm text-gray-600">
                        <code className="text-xs">{f.defaultValue}</code>
                      </td>
                      <td className="px-4 py-3">
                        <span className={`inline-flex rounded-full px-2 py-0.5 text-xs font-medium ${f.enabled ? 'bg-green-100 text-green-800' : 'bg-gray-100 text-gray-600'}`}>
                          {f.enabled ? 'On' : 'Off'}
                        </span>
                      </td>
                      <td className="px-4 py-3 text-sm text-gray-600">
                        {(f.rolloutPercentage * 100).toFixed(0)}%
                      </td>
                      <td className="px-4 py-3 text-sm text-gray-600">
                        {f.variants.length}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </>
      )}
    </div>
  );
}

export default function FlagListPage() {
  return <FlagListContent />;
}
