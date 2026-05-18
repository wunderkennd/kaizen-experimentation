'use client';

import { useEffect, useMemo, useState } from 'react';
import { listMetricDefinitions } from '@/lib/api';
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

  function updateWeight(metricId: string, weight: number) {
    onChange(value.map((op) => (op.metricId === metricId ? { ...op, weight } : op)));
  }

  return (
    <div className="flex flex-col gap-2" data-testid="operand-picker">
      {/* Search input + dropdown */}
      <input
        type="text"
        value={query}
        onChange={(e) => setQuery(e.target.value)}
        placeholder={loading ? 'Loading metrics…' : 'Search metric ID or name…'}
        disabled={disabled || loading}
        className="rounded border border-gray-300 px-3 py-2 text-sm focus:border-indigo-500 focus:outline-none"
      />
      {query && filteredCandidates.length > 0 && (
        <ul className="max-h-48 overflow-auto rounded border border-gray-200 bg-white shadow-sm">
          {filteredCandidates.map((m) => (
            <li key={m.metricId}>
              <button
                type="button"
                onClick={() => addOperand(m.metricId)}
                disabled={disabled}
                className="w-full px-3 py-2 text-left text-sm hover:bg-indigo-50"
              >
                <div className="font-mono text-xs text-gray-500">{m.metricId}</div>
                <div>{m.name}</div>
              </button>
            </li>
          ))}
        </ul>
      )}

      {/* Selected chips */}
      {value.length > 0 && (
        <ul className="flex flex-col gap-1" data-testid="selected-operands">
          {value.map((op) => (
            <li key={op.metricId} className="flex items-center gap-2 rounded bg-indigo-100 px-3 py-1 text-sm">
              <span className="flex-1 font-mono">{op.metricId}</span>
              {showWeights && (
                <input
                  type="number"
                  value={op.weight}
                  onChange={(e) => updateWeight(op.metricId, Number(e.target.value))}
                  min={0}
                  step={0.1}
                  disabled={disabled}
                  className="w-20 rounded border border-gray-300 px-2 py-0.5 text-xs"
                  aria-label={`weight for ${op.metricId}`}
                />
              )}
              <button
                type="button"
                onClick={() => removeOperand(op.metricId)}
                disabled={disabled}
                aria-label={`remove operand ${op.metricId}`}
                className="text-gray-500 hover:text-red-600"
              >
                ×
              </button>
            </li>
          ))}
        </ul>
      )}

      {value.length === 0 && !loading && (
        <p className="text-xs text-gray-500">no operands selected yet</p>
      )}
    </div>
  );
}
