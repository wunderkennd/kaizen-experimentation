'use client';

import { useEffect, useState, useCallback, useMemo } from 'react';
import Link from 'next/link';
import type { Flag, FlagType } from '@/lib/types';
import { listFlags } from '@/lib/api';
import { RetryableError } from '@/components/retryable-error';
import { useAuth } from '@/lib/auth-context';

const FLAG_TYPE_BADGE: Record<FlagType, string> = {
  BOOLEAN: 'bg-blue-100 text-blue-800',
  STRING: 'bg-green-100 text-green-800',
  NUMERIC: 'bg-purple-100 text-purple-800',
  JSON: 'bg-orange-100 text-orange-800',
};

function FlagListContent() {
  const { canAtLeast } = useAuth();
  const [flags, setFlags] = useState<Flag[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [search, setSearch] = useState('');

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
    if (!search) return flags;
    const q = search.toLowerCase();
    return flags.filter(
      (f) =>
        f.name.toLowerCase().includes(q) ||
        f.description.toLowerCase().includes(q),
    );
  }, [flags, search]);

  if (loading) {
    return (
      <div className="flex items-center justify-center py-12" role="status" aria-label="Loading">
        <div className="h-8 w-8 animate-spin rounded-full border-4 border-gray-300 border-t-indigo-600" />
        <span className="sr-only">Loading</span>
      </div>
    );
  }

  if (error) {
    return <RetryableError message={error} onRetry={fetchData} context="feature flags" />;
  }

  if (flags.length === 0) {
    return (
      <div className="py-12 text-center" data-testid="empty-state">
        <p className="text-sm text-gray-500">No feature flags found.</p>
        {canAtLeast('experimenter') && (
          <div className="mt-6">
            <Link
              href="/flags/new"
              className="rounded-md bg-indigo-600 px-4 py-2 text-sm font-medium text-white hover:bg-indigo-700"
              data-testid="create-first-flag"
            >
              Create your first feature flag
            </Link>
          </div>
        )}
      </div>
    );
  }

  return (
    <div>
      <div className="mb-6 flex items-center justify-between">
        <div className="flex items-center gap-3">
          <h1 className="text-2xl font-bold text-gray-900">Feature Flags</h1>
          <span className="inline-flex items-center rounded-full bg-gray-100 px-2.5 py-0.5 text-xs font-medium text-gray-700" data-testid="flag-count">
            {filtered.length}
          </span>
        </div>
        {canAtLeast('experimenter') && (
          <Link
            href="/flags/new"
            className="rounded-md bg-indigo-600 px-3 py-2 text-sm font-medium text-white shadow-sm hover:bg-indigo-500"
            data-testid="new-flag-button"
          >
            New Flag
          </Link>
        )}
      </div>

      <div className="mb-4 flex flex-wrap items-center gap-3">
        <div className="relative flex-1 min-w-[300px] max-w-md">
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
            type="text"
            placeholder="Search by name or description..."
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            className="w-full rounded-md border border-gray-300 py-1.5 pl-9 pr-3 text-sm shadow-sm placeholder:text-gray-400 focus:border-indigo-500 focus:outline-none focus:ring-1 focus:ring-indigo-500"
            data-testid="flag-search"
            aria-label="Search flags"
          />
        </div>
        {search && (
          <button
            onClick={() => setSearch('')}
            className="rounded-md border border-gray-300 px-3 py-1.5 text-sm text-gray-600 hover:bg-gray-50"
            data-testid="clear-search-toolbar"
          >
            Clear filters
          </button>
        )}
      </div>

      {filtered.length === 0 ? (
        <div className="py-12 text-center" data-testid="no-filter-matches">
          <p className="text-sm text-gray-500">No flags match your search.</p>
          <button
            onClick={() => setSearch('')}
            className="mt-2 text-sm text-indigo-600 hover:text-indigo-800"
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
                <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">Type</th>
                <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">Default</th>
                <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">Enabled</th>
                <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">Rollout %</th>
                <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">Variants</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-gray-200">
              {filtered.map((f) => (
                <tr key={f.flagId} className="hover:bg-gray-50" data-testid={`flag-row-${f.flagId}`}>
                  <td className="px-4 py-3">
                    <Link href={`/flags/${f.flagId}`} className="font-medium text-indigo-600 hover:text-indigo-800">
                      {f.name}
                    </Link>
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
    </div>
  );
}

export default function FlagListPage() {
  return <FlagListContent />;
}
