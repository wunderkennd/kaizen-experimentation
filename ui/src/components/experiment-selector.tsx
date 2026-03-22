'use client';

import { useState, useRef, useEffect, memo } from 'react';
import type { Experiment } from '@/lib/types';
import { STATE_CONFIG, TYPE_LABELS } from '@/lib/utils';

interface ExperimentSelectorProps {
  experiments: Experiment[];
  selectedIds: string[];
  onSelect: (id: string) => void;
  onRemove: (id: string) => void;
  maxSelections?: number;
}

function ExperimentSelectorInner({
  experiments,
  selectedIds,
  onSelect,
  onRemove,
  maxSelections = 4,
}: ExperimentSelectorProps) {
  const [query, setQuery] = useState('');
  const [isOpen, setIsOpen] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);

  // Filter to experiments with results (RUNNING or CONCLUDED)
  const selectableExperiments = experiments.filter(
    (e) =>
      (e.state === 'RUNNING' || e.state === 'CONCLUDED') &&
      !selectedIds.includes(e.experimentId),
  );

  const filteredExperiments = selectableExperiments.filter((e) =>
    e.name.toLowerCase().includes(query.toLowerCase()) ||
    e.ownerEmail.toLowerCase().includes(query.toLowerCase()),
  );

  const selectedExperiments = experiments.filter((e) =>
    selectedIds.includes(e.experimentId),
  );

  // Close dropdown when clicking outside
  useEffect(() => {
    function handleClickOutside(event: MouseEvent) {
      if (containerRef.current && !containerRef.current.contains(event.target as Node)) {
        setIsOpen(false);
      }
    }
    document.addEventListener('mousedown', handleClickOutside);
    return () => document.removeEventListener('mousedown', handleClickOutside);
  }, []);

  const atLimit = selectedIds.length >= maxSelections;

  return (
    <div className="mb-6">
      <label className="block text-sm font-medium text-gray-700 mb-2">
        Select experiments to compare (2-{maxSelections})
      </label>

      {/* Selected experiment chips */}
      {selectedExperiments.length > 0 && (
        <div className="mb-3 flex flex-wrap gap-2" data-testid="selected-experiments">
          {selectedExperiments.map((exp) => {
            const stateConfig = STATE_CONFIG[exp.state];
            return (
              <span
                key={exp.experimentId}
                className={`inline-flex items-center gap-1.5 rounded-full px-3 py-1 text-sm font-medium ${stateConfig.bgColor} ${stateConfig.textColor}`}
              >
                {exp.name}
                <button
                  type="button"
                  onClick={() => onRemove(exp.experimentId)}
                  className="ml-1 inline-flex h-4 w-4 items-center justify-center rounded-full hover:bg-black/10"
                  aria-label={`Remove ${exp.name}`}
                >
                  x
                </button>
              </span>
            );
          })}
        </div>
      )}

      {/* Search dropdown */}
      <div ref={containerRef} className="relative">
        <input
          type="text"
          value={query}
          onChange={(e) => {
            setQuery(e.target.value);
            setIsOpen(true);
          }}
          onFocus={() => setIsOpen(true)}
          placeholder={atLimit ? `Maximum ${maxSelections} experiments selected` : 'Search experiments by name or owner...'}
          disabled={atLimit}
          className="w-full rounded-md border border-gray-300 px-3 py-2 text-sm shadow-sm focus:border-indigo-500 focus:outline-none focus:ring-1 focus:ring-indigo-500 disabled:bg-gray-100 disabled:cursor-not-allowed"
          aria-label="Search experiments"
          data-testid="experiment-search"
        />

        {isOpen && !atLimit && (
          <ul
            className="absolute z-10 mt-1 max-h-60 w-full overflow-auto rounded-md border border-gray-200 bg-white shadow-lg"
            role="listbox"
            data-testid="experiment-dropdown"
          >
            {filteredExperiments.length === 0 ? (
              <li className="px-3 py-2 text-sm text-gray-500">
                No matching experiments with results available
              </li>
            ) : (
              filteredExperiments.map((exp) => {
                const stateConfig = STATE_CONFIG[exp.state];
                return (
                  <li
                    key={exp.experimentId}
                    role="option"
                    aria-selected={false}
                    onClick={() => {
                      onSelect(exp.experimentId);
                      setQuery('');
                      if (selectedIds.length + 1 >= maxSelections) {
                        setIsOpen(false);
                      }
                    }}
                    className="cursor-pointer px-3 py-2 hover:bg-indigo-50 border-b border-gray-100 last:border-b-0"
                  >
                    <div className="flex items-center justify-between">
                      <div>
                        <span className="text-sm font-medium text-gray-900">{exp.name}</span>
                        <span className="ml-2 text-xs text-gray-500">{TYPE_LABELS[exp.type]}</span>
                      </div>
                      <span className={`inline-flex items-center rounded-full px-2 py-0.5 text-xs font-medium ${stateConfig.bgColor} ${stateConfig.textColor}`}>
                        {stateConfig.label}
                      </span>
                    </div>
                    <div className="text-xs text-gray-400 mt-0.5">{exp.ownerEmail}</div>
                  </li>
                );
              })
            )}
          </ul>
        )}
      </div>
    </div>
  );
}

export const ExperimentSelector = memo(ExperimentSelectorInner);
