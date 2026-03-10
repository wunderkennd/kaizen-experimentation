'use client';

import { useEffect, useState } from 'react';
import Link from 'next/link';
import type { Experiment } from '@/lib/types';
import { listExperiments } from '@/lib/api';
import { ExperimentCard } from '@/components/experiment-card';
import { ExperimentFiltersToolbar } from '@/components/experiment-filters';
import { useExperimentFilters, type SortField } from '@/lib/use-experiment-filters';
import { useAuth } from '@/lib/auth-context';
import { ROLE_LABELS } from '@/lib/auth';

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
      className="cursor-pointer select-none px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500 hover:text-gray-700"
      onClick={() => onToggle(field)}
      aria-sort={isActive ? (currentDir === 'asc' ? 'ascending' : 'descending') : 'none'}
    >
      <span className="inline-flex items-center gap-1">
        {label}
        {isActive && (
          <span className="text-indigo-600">{currentDir === 'asc' ? '\u25B2' : '\u25BC'}</span>
        )}
        {!isActive && (
          <span className="text-gray-300">{'\u25B2'}</span>
        )}
      </span>
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

  useEffect(() => {
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

  if (loading) {
    return (
      <div className="flex items-center justify-center py-12">
        <div className="h-8 w-8 animate-spin rounded-full border-4 border-gray-300 border-t-indigo-600" />
      </div>
    );
  }

  if (error) {
    return (
      <div className="rounded-md bg-red-50 p-4">
        <p className="text-sm text-red-700">Failed to load experiments: {error}</p>
      </div>
    );
  }

  if (experiments.length === 0) {
    return (
      <div className="py-12 text-center">
        <p className="text-sm text-gray-500">No experiments yet. Create your first experiment to get started.</p>
      </div>
    );
  }

  const filtered = filters.applyFilters(experiments);

  return (
    <div>
      <div className="mb-6 flex items-center justify-between">
        <h1 className="text-2xl font-bold text-gray-900">Experiments</h1>
        {canCreate ? (
          <Link
            href="/experiments/new"
            className="rounded-md bg-indigo-600 px-3 py-2 text-sm font-medium text-white shadow-sm hover:bg-indigo-500"
            data-testid="new-experiment-link"
          >
            New Experiment
          </Link>
        ) : (
          <span
            className="rounded-md bg-indigo-600 px-3 py-2 text-sm font-medium text-white opacity-50 cursor-not-allowed"
            title={`Requires Experimenter role (you are ${ROLE_LABELS[user.role]})`}
            data-testid="new-experiment-disabled"
          >
            New Experiment
          </span>
        )}
      </div>

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
                <ExperimentCard key={exp.experimentId} experiment={exp} />
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}
