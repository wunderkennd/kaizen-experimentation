'use client';

import { useCallback, useMemo, useRef, useState } from 'react';
import type { Experiment, ExperimentState, ExperimentType } from './types';

export type SortField = 'name' | 'type' | 'state' | 'createdAt';
export type SortDir = 'asc' | 'desc';

export interface ExperimentFilters {
  query: string;
  stateFilter: ExperimentState | '';
  typeFilter: ExperimentType | '';
  sortField: SortField;
  sortDir: SortDir;
  setQuery(q: string): void;
  setStateFilter(s: ExperimentState | ''): void;
  setTypeFilter(t: ExperimentType | ''): void;
  toggleSort(field: SortField): void;
  clearFilters(): void;
  applyFilters(experiments: Experiment[]): Experiment[];
  hasActiveFilters: boolean;
}

const STATE_ORDER: Record<ExperimentState, number> = {
  DRAFT: 0,
  STARTING: 1,
  RUNNING: 2,
  CONCLUDING: 3,
  CONCLUDED: 4,
  ARCHIVED: 5,
};

export function useExperimentFilters(): ExperimentFilters {
  const [query, setQueryRaw] = useState('');
  const [stateFilter, setStateFilter] = useState<ExperimentState | ''>('');
  const [typeFilter, setTypeFilter] = useState<ExperimentType | ''>('');
  const [sortField, setSortField] = useState<SortField>('createdAt');
  const [sortDir, setSortDir] = useState<SortDir>('desc');

  // Debounce search input
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const [debouncedQuery, setDebouncedQuery] = useState('');

  const setQuery = useCallback((q: string) => {
    setQueryRaw(q);
    if (debounceRef.current) clearTimeout(debounceRef.current);
    debounceRef.current = setTimeout(() => setDebouncedQuery(q), 300);
  }, []);

  const toggleSort = useCallback((field: SortField) => {
    setSortField((prev) => {
      if (prev === field) {
        setSortDir((d) => (d === 'asc' ? 'desc' : 'asc'));
        return prev;
      }
      setSortDir('asc');
      return field;
    });
  }, []);

  const clearFilters = useCallback(() => {
    setQueryRaw('');
    setDebouncedQuery('');
    setStateFilter('');
    setTypeFilter('');
    setSortField('createdAt');
    setSortDir('desc');
    if (debounceRef.current) clearTimeout(debounceRef.current);
  }, []);

  const hasActiveFilters = query !== '' || stateFilter !== '' || typeFilter !== '';

  const applyFilters = useCallback(
    (experiments: Experiment[]): Experiment[] => {
      let filtered = experiments;

      // Text search (case-insensitive substring on name, owner, description)
      if (debouncedQuery) {
        const q = debouncedQuery.toLowerCase();
        filtered = filtered.filter(
          (e) =>
            e.name.toLowerCase().includes(q) ||
            e.ownerEmail.toLowerCase().includes(q) ||
            e.description.toLowerCase().includes(q) ||
            e.experimentId.toLowerCase().includes(q),
        );
      }

      // State filter
      if (stateFilter) {
        filtered = filtered.filter((e) => e.state === stateFilter);
      }

      // Type filter
      if (typeFilter) {
        filtered = filtered.filter((e) => e.type === typeFilter);
      }

      // Sort
      const sorted = [...filtered].sort((a, b) => {
        let cmp = 0;
        switch (sortField) {
          case 'name':
            cmp = a.name.localeCompare(b.name);
            break;
          case 'type':
            cmp = a.type.localeCompare(b.type);
            break;
          case 'state':
            cmp = STATE_ORDER[a.state] - STATE_ORDER[b.state];
            break;
          case 'createdAt':
            cmp = new Date(a.createdAt).getTime() - new Date(b.createdAt).getTime();
            break;
        }
        return sortDir === 'asc' ? cmp : -cmp;
      });

      return sorted;
    },
    [debouncedQuery, stateFilter, typeFilter, sortField, sortDir],
  );

  return useMemo(
    () => ({
      query,
      stateFilter,
      typeFilter,
      sortField,
      sortDir,
      setQuery,
      setStateFilter,
      setTypeFilter,
      toggleSort,
      clearFilters,
      applyFilters,
      hasActiveFilters,
    }),
    [query, stateFilter, typeFilter, sortField, sortDir, setQuery, toggleSort, clearFilters, applyFilters, hasActiveFilters],
  );
}
