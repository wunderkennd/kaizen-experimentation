'use client';

import { useRef } from 'react';
import type { AuditAction } from '@/lib/types';
import { useSearchShortcut } from '@/hooks/use-search-shortcut';

const ALL_ACTIONS: AuditAction[] = [
  'CREATED', 'UPDATED', 'STARTED', 'PAUSED', 'RESUMED',
  'CONCLUDED', 'ARCHIVED', 'GUARDRAIL_BREACH', 'CONFIG_CHANGED',
];

const ACTION_LABELS: Record<AuditAction, string> = {
  CREATED: 'Created',
  UPDATED: 'Updated',
  STARTED: 'Started',
  PAUSED: 'Paused',
  RESUMED: 'Resumed',
  CONCLUDED: 'Concluded',
  ARCHIVED: 'Archived',
  GUARDRAIL_BREACH: 'Guardrail Breach',
  CONFIG_CHANGED: 'Config Changed',
};

interface AuditFiltersProps {
  experimentQuery: string;
  onExperimentQueryChange: (value: string) => void;
  actionFilter: AuditAction | '';
  onActionFilterChange: (value: AuditAction | '') => void;
  actorQuery: string;
  onActorQueryChange: (value: string) => void;
  totalCount: number;
  filteredCount: number;
  onClear: () => void;
  hasActiveFilters: boolean;
}

export function AuditFilters({
  experimentQuery,
  onExperimentQueryChange,
  actionFilter,
  onActionFilterChange,
  actorQuery,
  onActorQueryChange,
  totalCount,
  filteredCount,
  onClear,
  hasActiveFilters,
}: AuditFiltersProps) {
  const inputRef = useRef<HTMLInputElement>(null);
  useSearchShortcut(inputRef);

  return (
    <div className="mb-4 flex flex-wrap items-center gap-3">
      {/* Experiment name search */}
      <div className="group relative flex-1 min-w-[200px] max-w-sm">
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
          placeholder="Search by experiment..."
          value={experimentQuery}
          onChange={(e) => onExperimentQueryChange(e.target.value)}
          className="w-full rounded-md border border-gray-300 py-1.5 pl-9 pr-10 text-sm placeholder:text-gray-400 focus:border-indigo-500 focus:outline-none focus:ring-1 focus:ring-indigo-500"
          aria-label="Search by experiment name"
        />
        {experimentQuery ? (
          <button
            type="button"
            onClick={() => onExperimentQueryChange('')}
            className="absolute right-3 top-1/2 -translate-y-1/2 text-gray-400 hover:text-gray-600 focus:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500 rounded-sm"
            aria-label="Clear experiment search"
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

      {/* Action filter */}
      <select
        value={actionFilter}
        onChange={(e) => onActionFilterChange(e.target.value as AuditAction | '')}
        className="rounded-md border border-gray-300 py-1.5 pl-3 pr-8 text-sm focus:border-indigo-500 focus:outline-none focus:ring-1 focus:ring-indigo-500"
        aria-label="Filter by action"
      >
        <option value="">All Actions</option>
        {ALL_ACTIONS.map((a) => (
          <option key={a} value={a}>{ACTION_LABELS[a]}</option>
        ))}
      </select>

      {/* Actor filter */}
      <div className="group relative max-w-[200px]">
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
          placeholder="Filter by actor..."
          value={actorQuery}
          onChange={(e) => onActorQueryChange(e.target.value)}
          className="w-full rounded-md border border-gray-300 py-1.5 pl-9 pr-10 text-sm placeholder:text-gray-400 focus:border-indigo-500 focus:outline-none focus:ring-1 focus:ring-indigo-500"
          aria-label="Filter by actor email"
        />
        {actorQuery && (
          <button
            type="button"
            onClick={() => onActorQueryChange('')}
            className="absolute right-3 top-1/2 -translate-y-1/2 text-gray-400 hover:text-gray-600 focus:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500 rounded-sm"
            aria-label="Clear actor search"
            data-testid="clear-actor-search"
          >
            <svg className="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" aria-hidden="true">
              <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M6 18L18 6M6 6l12 12" />
            </svg>
          </button>
        )}
      </div>

      {/* Clear filters */}
      {hasActiveFilters && (
        <button
          onClick={onClear}
          className="rounded-md border border-gray-300 px-3 py-1.5 text-sm text-gray-600 hover:bg-gray-50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500"
          data-testid="clear-filters-toolbar"
        >
          Clear filters
        </button>
      )}

      {/* Count badge */}
      <span className="ml-auto text-sm text-gray-500" data-testid="audit-count">
        Showing {filteredCount} of {totalCount} entries
      </span>
    </div>
  );
}
