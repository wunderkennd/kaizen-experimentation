'use client';

import { useEffect, useState, useCallback } from 'react';
import Link from 'next/link';
import type { Experiment } from '@/lib/types';
import { listExperiments } from '@/lib/api';
import { ExperimentRow } from '@/components/experiment-card';
import { ExperimentFiltersToolbar } from '@/components/experiment-filters';
import { useExperimentFilters, type SortField } from '@/lib/use-experiment-filters';
import { useAuth } from '@/lib/auth-context';
import { ROLE_LABELS } from '@/lib/auth';
import { RetryableError } from '@/components/retryable-error';
import { Breadcrumb } from '@/components/breadcrumb';

function SortableHeader({
  label,
  field,
  currentField,
  currentDir,
  onToggle,
}: {
  label: string;
  field: SortField;
  currentField: SortField;
  currentDir: 'asc' | 'desc';
  onToggle: (f: SortField) => void;
}) {
  const isActive = currentField === field;
  return (
    <th
      className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500"
      aria-sort={isActive ? (currentDir === 'asc' ? 'ascending' : 'descending') : 'none'}
    >
      <button
        type="button"
        onClick={() => onToggle(field)}
        className="inline-flex cursor-pointer select-none items-center gap-1 hover:text-gray-700 focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-indigo-500 outline-none rounded-sm"
        title={`Sort by ${label}`}
      >
        {label}
        {isActive ? (
          currentDir === 'asc' ? (
            <svg className="ml-1 h-3 w-3 text-indigo-600" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={3} aria-hidden="true">
              <path strokeLinecap="round" strokeLinejoin="round" d="M5 15l7-7 7 7" />
            </svg>
          ) : (
            <svg className="ml-1 h-3 w-3 text-indigo-600" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={3} aria-hidden="true">
              <path strokeLinecap="round" strokeLinejoin="round" d="M19 9l-7 7-7-7" />
            </svg>
          )
        ) : (
          <svg className="ml-1 h-3 w-3 text-gray-300" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2} aria-hidden="true">
            <path strokeLinecap="round" strokeLinejoin="round" d="M7 16V4m0 0L3 8m4-4l4 4m6 0v12m0 0l4-4m-4 4l-4-4" />
          </svg>
        )}
      </button>
    </th>
  );
}

export default function ExperimentListPage() {
  const [experiments, setExperiments] = useState<Experiment[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const filters = useExperimentFilters();
  const { canAtLeast, user } = useAuth();
  const canCreate = canAtLeast('experimenter');

  const fetchData = useCallback(() => {
    setLoading(true);
    setError(null);
    listExperiments()
      .then((data) => {
        setExperiments(data.experiments);
      })
      .catch((err) => {
        setError(err.message);
      })
      .finally(() => {
        setLoading(false);
      });
  }, []);

  useEffect(() => { fetchData(); }, [fetchData]);

  const filtered = filters.applyFilters(experiments);

  return (
    <div>
      <Breadcrumb items={[{ label: 'Experiments' }]} />

      <div className="mb-6 flex items-center justify-between">
        <h1 className="text-2xl font-bold text-gray-900">Experiments</h1>
        {!loading && !error && experiments.length > 0 && (
          canCreate ? (
            <Link
              href="/experiments/new"
              className="rounded-md bg-indigo-600 px-3 py-2 text-sm font-medium text-white shadow-sm hover:bg-indigo-500 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500 focus-visible:ring-offset-2"
              data-testid="new-experiment-link"
            >
              New Experiment
            </Link>
          ) : (
            <span
              className="rounded-md bg-indigo-600 px-3 py-2 text-sm font-medium text-white opacity-50 cursor-not-allowed focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500 focus-visible:ring-offset-2"
              title={`Requires Experimenter role (you are ${ROLE_LABELS[user.role]})`}
              data-testid="new-experiment-disabled"
              tabIndex={0}
              role="button"
              aria-disabled="true"
            >
              New Experiment
            </span>
          )
        )}
      </div>

      {loading ? (
        <div className="flex items-center justify-center py-12" role="status" aria-label="Loading">
          <div className="h-8 w-8 animate-spin rounded-full border-4 border-gray-300 border-t-indigo-600" />
          <span className="sr-only">Loading</span>
        </div>
      ) : error ? (
        <RetryableError message={error} onRetry={fetchData} context="experiments" />
      ) : experiments.length === 0 ? (
        <div className="py-12 text-center" data-testid="empty-state">
          <p className="text-sm text-gray-500">No experiments yet.</p>
          {canCreate && (
            <div className="mt-6">
              <Link
                href="/experiments/new"
                className="rounded-md bg-indigo-600 px-4 py-2 text-sm font-medium text-white hover:bg-indigo-700 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500 focus-visible:ring-offset-2"
                data-testid="create-first-experiment"
              >
                Create your first experiment
              </Link>
            </div>
          )}
        </div>
      ) : (
        <>
      <ExperimentFiltersToolbar
        filters={filters}
        totalCount={experiments.length}
        filteredCount={filtered.length}
      />

      {filtered.length === 0 ? (
        <div className="py-12 text-center" data-testid="no-filter-matches">
          <p className="text-sm text-gray-500">No experiments match your filters.</p>
          <button
            onClick={filters.clearFilters}
            className="mt-2 rounded-sm text-sm text-indigo-600 hover:text-indigo-800 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500"
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
                <SortableHeader
                  label="Name"
                  field="name"
                  currentField={filters.sortField}
                  currentDir={filters.sortDir}
                  onToggle={filters.toggleSort}
                />
                <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">
                  Owner
                </th>
                <SortableHeader
                  label="Type"
                  field="type"
                  currentField={filters.sortField}
                  currentDir={filters.sortDir}
                  onToggle={filters.toggleSort}
                />
                <SortableHeader
                  label="State"
                  field="state"
                  currentField={filters.sortField}
                  currentDir={filters.sortDir}
                  onToggle={filters.toggleSort}
                />
                <SortableHeader
                  label="Created"
                  field="createdAt"
                  currentField={filters.sortField}
                  currentDir={filters.sortDir}
                  onToggle={filters.toggleSort}
                />
                <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">
                  Results
                </th>
              </tr>
            </thead>
            <tbody className="divide-y divide-gray-200">
              {filtered.map((exp) => (
                <ExperimentRow key={exp.experimentId} experiment={exp} />
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
