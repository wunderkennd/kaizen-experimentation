'use client';

import { useState, useRef, useEffect, memo } from 'react';
import { useSearchShortcut } from '@/hooks/use-search-shortcut';
import type { Experiment } from '@/lib/types';
import { STATE_CONFIG, TYPE_LABELS } from '@/lib/utils';

interface ExperimentSelectorProps {
  experiments: Experiment[];
  selectedIds: string[];
  onSelect: (id: string) => void;
  onRemove: (id: string) => void;
  onClearAll?: () => void;
  maxSelections?: number;
}

function ExperimentSelectorInner({
  experiments,
  selectedIds,
  onSelect,
  onRemove,
  onClearAll,
  maxSelections = 4,
}: ExperimentSelectorProps) {
  const [query, setQuery] = useState('');
  const [isOpen, setIsOpen] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  useSearchShortcut(inputRef);

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
      <label htmlFor="experiment-search" className="block text-sm font-medium text-gray-700 mb-2">
        Select experiments to compare (2-{maxSelections})
      </label>

      {/* Selected experiment chips */}
      {selectedExperiments.length > 0 && (
        <div className="mb-3 flex flex-wrap items-center gap-2" data-testid="selected-experiments">
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
                  className="ml-1 inline-flex h-4 w-4 items-center justify-center rounded-full hover:bg-black/10 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500 focus-visible:ring-offset-1"
                  aria-label={`Remove ${exp.name}`}
                >
                  x
                </button>
              </span>
            );
          })}
          {onClearAll && (
            <button
              type="button"
              onClick={onClearAll}
              className="rounded-full border border-gray-300 bg-white px-3 py-1 text-xs font-medium text-gray-600 hover:bg-gray-50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500 focus-visible:ring-offset-1"
              data-testid="clear-all-selections"
              aria-label="Clear all selections"
            >
              Clear all
            </button>
          )}
        </div>
      )}

      {/* Search dropdown */}
      <div ref={containerRef} className="relative">
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
            id="experiment-search"
            type="text"
            value={query}
            onChange={(e) => {
              setQuery(e.target.value);
              setIsOpen(true);
            }}
            onFocus={() => setIsOpen(true)}
            placeholder={atLimit ? `Maximum ${maxSelections} experiments selected` : 'Search experiments by name or owner...'}
            disabled={atLimit}
            className="w-full rounded-md border border-gray-300 py-2 pl-9 pr-10 text-sm shadow-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500 focus-visible:ring-offset-2 disabled:bg-gray-100 disabled:cursor-not-allowed"
            aria-label="Search experiments"
            data-testid="experiment-search"
          />
          {query ? (
            <button
              type="button"
              onClick={() => {
                setQuery('');
              }}
              className="absolute right-3 top-1/2 -translate-y-1/2 text-gray-400 hover:text-gray-600 focus:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500 rounded-sm"
              aria-label="Clear search"
              data-testid="clear-search-button"
            >
              <svg className="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" aria-hidden="true">
                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M6 18L18 6M6 6l12 12" />
              </svg>
            </button>
          ) : !atLimit ? (
            <div className="pointer-events-none absolute right-3 top-1/2 flex -translate-y-1/2 items-center group-focus-within:hidden group-hover:hidden">
              <span className="flex h-5 w-5 items-center justify-center rounded border border-gray-300 bg-gray-50 text-[10px] font-medium text-gray-500">
                /
              </span>
            </div>
          ) : null}
        </div>

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
                    tabIndex={0}
                    onClick={() => {
                      onSelect(exp.experimentId);
                      setQuery('');
                      if (selectedIds.length + 1 >= maxSelections) {
                        setIsOpen(false);
                      }
                    }}
                    onKeyDown={(e) => {
                      if (e.key === 'Enter' || e.key === ' ') {
                        e.preventDefault();
                        onSelect(exp.experimentId);
                        setQuery('');
                        if (selectedIds.length + 1 >= maxSelections) {
                          setIsOpen(false);
                        }
                      }
                    }}
                    className="cursor-pointer px-3 py-2 hover:bg-indigo-50 focus:bg-indigo-50 focus:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-indigo-500 border-b border-gray-100 last:border-b-0"
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
