'use client';

import { useEffect, useMemo, useRef, useState } from 'react';
import { listMetricDefinitions } from '@/lib/api';
import { useSearchShortcut } from '@/hooks/use-search-shortcut';
import type { CompositeOperand, MetricDefinition } from '@/lib/types';

interface OperandPickerProps {
  value: CompositeOperand[];
  onChange: (next: CompositeOperand[]) => void;
  showWeights: boolean;  // true only when operator === WEIGHTED_SUM
  disabled?: boolean;
}

export function OperandPicker({ value, onChange, showWeights, disabled }: OperandPickerProps) {
  const [candidates, setCandidates] = useState<MetricDefinition[]>([]);
  const [loading, setLoading] = useState(true);
  const [query, setQuery] = useState('');
  const inputRef = useRef<HTMLInputElement>(null);

  useSearchShortcut(inputRef);

  useEffect(() => {
    listMetricDefinitions()
      .then((resp) => setCandidates(resp.metrics ?? []))
      .catch(() => setCandidates([]))
      .finally(() => setLoading(false));
  }, []);

  // Don't show already-selected operands in the candidate list.
  const selectedIds = useMemo(() => new Set(value.map((op) => op.metricId)), [value]);
  const filteredCandidates = useMemo(() => {
    const q = query.trim().toLowerCase();
    return candidates
      .filter((m) => !selectedIds.has(m.metricId))
      .filter((m) =>
        q === '' ||
        m.metricId.toLowerCase().includes(q) ||
        m.name.toLowerCase().includes(q)
      )
      .slice(0, 20);  // cap candidate list to avoid huge dropdowns
  }, [candidates, selectedIds, query]);

  function addOperand(metricId: string) {
    onChange([...value, { metricId, weight: showWeights ? 1.0 : 0 }]);
    setQuery('');
  }

  function removeOperand(metricId: string) {
    onChange(value.filter((op) => op.metricId !== metricId));
  }

  function clearAllOperands() {
    onChange([]);
  }

  function updateWeight(metricId: string, weight: number) {
    onChange(value.map((op) => (op.metricId === metricId ? { ...op, weight } : op)));
  }

  return (
    <div className="flex flex-col gap-2" data-testid="operand-picker">
      {/* Standardized Search input + dropdown */}
      <div className="group relative w-full">
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
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder={loading ? 'Loading metrics…' : 'Search metric ID or name…'}
          disabled={disabled || loading}
          aria-label="Search operands"
          className="w-full rounded border border-gray-300 py-2 pl-9 pr-10 text-sm focus:border-indigo-500 focus:outline-none focus:ring-1 focus:ring-indigo-500 disabled:bg-gray-50"
        />
        {query ? (
          <button
            type="button"
            onClick={() => {
              setQuery('');
              inputRef.current?.focus();
            }}
            disabled={disabled}
            className="absolute right-3 top-1/2 -translate-y-1/2 rounded-sm text-gray-400 hover:text-gray-600 focus:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500"
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

      {query && filteredCandidates.length > 0 && (
        <ul className="max-h-48 overflow-auto rounded border border-gray-200 bg-white shadow-sm">
          {filteredCandidates.map((m) => (
            <li key={m.metricId}>
              <button
                type="button"
                onClick={() => addOperand(m.metricId)}
                disabled={disabled}
                className="w-full px-3 py-2 text-left text-sm hover:bg-indigo-50 focus:bg-indigo-50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-indigo-500"
              >
                <div className="font-mono text-xs text-gray-500">{m.metricId}</div>
                <div>{m.name}</div>
              </button>
            </li>
          ))}
        </ul>
      )}

      {/* Selected chips + Clear all button */}
      {value.length > 0 && (
        <div className="flex flex-col gap-1.5">
          <div className="flex items-center justify-between">
            <span className="text-xs font-medium text-gray-600">Selected Operands ({value.length})</span>
            {value.length > 1 && (
              <button
                type="button"
                onClick={clearAllOperands}
                disabled={disabled}
                className="rounded-sm text-xs font-medium text-indigo-600 hover:text-indigo-800 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500 focus-visible:ring-offset-1 disabled:opacity-50"
                data-testid="clear-all-operands-button"
              >
                Clear all
              </button>
            )}
          </div>
          <ul className="flex flex-col gap-1" data-testid="selected-operands">
            {value.map((op) => (
              <li key={op.metricId} className="flex items-center gap-2 rounded bg-indigo-100 px-3 py-1 text-sm">
                <span className="flex-1 font-mono text-indigo-950">{op.metricId}</span>
                {showWeights && (
                  <input
                    type="number"
                    value={op.weight}
                    onChange={(e) => updateWeight(op.metricId, Number(e.target.value))}
                    min={0}
                    step={0.1}
                    disabled={disabled}
                    className="w-20 rounded border border-gray-300 bg-white px-2 py-0.5 text-xs focus:border-indigo-500 focus:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500"
                    aria-label={`weight for ${op.metricId}`}
                  />
                )}
                <button
                  type="button"
                  onClick={() => removeOperand(op.metricId)}
                  disabled={disabled}
                  aria-label={`remove operand ${op.metricId}`}
                  className="rounded-sm text-gray-500 hover:text-red-600 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500"
                >
                  ×
                </button>
              </li>
            ))}
          </ul>
        </div>
      )}

      {value.length === 0 && !loading && (
        <p className="text-xs text-gray-500">No operands selected yet</p>
      )}
    </div>
  );
}
