'use client';

import type { ExperimentState, ExperimentType } from '@/lib/types';
import { STATE_CONFIG, TYPE_LABELS } from '@/lib/utils';
import type { ExperimentFilters as Filters } from '@/lib/use-experiment-filters';

interface ExperimentFiltersProps {
  filters: Filters;
  totalCount: number;
  filteredCount: number;
}

const ALL_STATES: ExperimentState[] = [
  'DRAFT', 'STARTING', 'RUNNING', 'CONCLUDING', 'CONCLUDED', 'ARCHIVED',
];

const ALL_TYPES: ExperimentType[] = [
  'AB', 'MULTIVARIATE', 'INTERLEAVING', 'SESSION_LEVEL', 'PLAYBACK_QOE',
  'MAB', 'CONTEXTUAL_BANDIT', 'CUMULATIVE_HOLDOUT',
];

export function ExperimentFiltersToolbar({ filters, totalCount, filteredCount }: ExperimentFiltersProps) {
  return (
    <div className="mb-4 flex flex-wrap items-center gap-3">
      {/* Search input */}
      <div className="relative flex-1 min-w-[200px] max-w-sm">
        <svg
          className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-gray-400"
          fill="none"
          stroke="currentColor"
          viewBox="0 0 24 24"
        >
          <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z" />
        </svg>
        <input
          type="text"
          placeholder="Search experiments..."
          value={filters.query}
          onChange={(e) => filters.setQuery(e.target.value)}
          className="w-full rounded-md border border-gray-300 py-1.5 pl-9 pr-3 text-sm placeholder:text-gray-400 focus:border-indigo-500 focus:outline-none focus:ring-1 focus:ring-indigo-500"
          aria-label="Search experiments"
        />
      </div>

      {/* State filter */}
      <select
        value={filters.stateFilter}
        onChange={(e) => filters.setStateFilter(e.target.value as ExperimentState | '')}
        className="rounded-md border border-gray-300 py-1.5 pl-3 pr-8 text-sm focus:border-indigo-500 focus:outline-none focus:ring-1 focus:ring-indigo-500"
        aria-label="Filter by state"
      >
        <option value="">All States</option>
        {ALL_STATES.map((s) => (
          <option key={s} value={s}>{STATE_CONFIG[s].label}</option>
        ))}
      </select>

      {/* Type filter */}
      <select
        value={filters.typeFilter}
        onChange={(e) => filters.setTypeFilter(e.target.value as ExperimentType | '')}
        className="rounded-md border border-gray-300 py-1.5 pl-3 pr-8 text-sm focus:border-indigo-500 focus:outline-none focus:ring-1 focus:ring-indigo-500"
        aria-label="Filter by type"
      >
        <option value="">All Types</option>
        {ALL_TYPES.map((t) => (
          <option key={t} value={t}>{TYPE_LABELS[t]}</option>
        ))}
      </select>

      {/* Clear filters */}
      {filters.hasActiveFilters && (
        <button
          onClick={filters.clearFilters}
          className="rounded-md border border-gray-300 px-3 py-1.5 text-sm text-gray-600 hover:bg-gray-50"
        >
          Clear filters
        </button>
      )}

      {/* Count badge */}
      <span className="ml-auto text-sm text-gray-500" data-testid="filter-count">
        Showing {filteredCount} of {totalCount} experiments
      </span>
    </div>
  );
}
