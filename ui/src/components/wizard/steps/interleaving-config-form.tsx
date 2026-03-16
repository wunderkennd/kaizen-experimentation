'use client';

import { useWizard } from '../wizard-context';
import type { InterleavingMethod, CreditAssignment } from '@/lib/types';

const METHODS: { value: InterleavingMethod; label: string }[] = [
  { value: 'TEAM_DRAFT', label: 'Team Draft' },
  { value: 'OPTIMIZED', label: 'Optimized' },
  { value: 'MULTILEAVE', label: 'Multileave' },
];

const CREDIT_ASSIGNMENTS: { value: CreditAssignment; label: string }[] = [
  { value: 'BINARY_WIN', label: 'Binary Win' },
  { value: 'PROPORTIONAL', label: 'Proportional' },
  { value: 'WEIGHTED', label: 'Weighted' },
];

export function InterleavingConfigForm() {
  const { state, dispatch } = useWizard();
  const config = state.interleavingConfig;

  const update = (partial: Partial<typeof config>) =>
    dispatch({ type: 'SET_FIELD', field: 'interleavingConfig', value: { ...config, ...partial } });

  const updateAlgorithmId = (index: number, value: string) => {
    const ids = [...config.algorithmIds];
    ids[index] = value;
    update({ algorithmIds: ids });
  };

  const addAlgorithmId = () => update({ algorithmIds: [...config.algorithmIds, ''] });

  const removeAlgorithmId = (index: number) =>
    update({ algorithmIds: config.algorithmIds.filter((_, i) => i !== index) });

  return (
    <div className="space-y-4">
      <div>
        <label htmlFor="interleave-method" className="block text-sm font-medium text-gray-700">
          Interleaving Method <span className="text-red-500">*</span>
        </label>
        <select
          id="interleave-method"
          value={config.method}
          onChange={(e) => update({ method: e.target.value as InterleavingMethod })}
          className="mt-1 block w-full rounded-md border border-gray-300 px-3 py-2 text-sm shadow-sm focus:border-indigo-500 focus:ring-indigo-500"
        >
          {METHODS.map((m) => (
            <option key={m.value} value={m.value}>{m.label}</option>
          ))}
        </select>
      </div>

      <div>
        <div className="mb-2 flex items-center justify-between">
          <label className="block text-sm font-medium text-gray-700">
            Algorithm IDs <span className="text-red-500">*</span>
          </label>
          <button
            type="button"
            onClick={addAlgorithmId}
            className="rounded-md border border-gray-300 bg-white px-2 py-1 text-xs font-medium text-gray-700 hover:bg-gray-50"
          >
            Add Algorithm
          </button>
        </div>
        {config.algorithmIds.map((id, i) => (
          <div key={i} className="mb-2 flex gap-2">
            <input
              type="text"
              value={id}
              onChange={(e) => updateAlgorithmId(i, e.target.value)}
              placeholder={`Algorithm ${i + 1} ID`}
              aria-label={`Algorithm ${i + 1} ID`}
              className="block w-full rounded-md border border-gray-300 px-3 py-2 text-sm shadow-sm focus:border-indigo-500 focus:ring-indigo-500"
            />
            {config.algorithmIds.length > 2 && (
              <button
                type="button"
                onClick={() => removeAlgorithmId(i)}
                className="text-sm text-red-600 hover:text-red-800"
              >
                Remove
              </button>
            )}
          </div>
        ))}
      </div>

      <div>
        <label htmlFor="credit-assignment" className="block text-sm font-medium text-gray-700">
          Credit Assignment
        </label>
        <select
          id="credit-assignment"
          value={config.creditAssignment}
          onChange={(e) => update({ creditAssignment: e.target.value as CreditAssignment })}
          className="mt-1 block w-full rounded-md border border-gray-300 px-3 py-2 text-sm shadow-sm focus:border-indigo-500 focus:ring-indigo-500"
        >
          {CREDIT_ASSIGNMENTS.map((c) => (
            <option key={c.value} value={c.value}>{c.label}</option>
          ))}
        </select>
      </div>

      <div>
        <label htmlFor="credit-metric" className="block text-sm font-medium text-gray-700">
          Credit Metric Event <span className="text-red-500">*</span>
        </label>
        <input
          id="credit-metric"
          type="text"
          value={config.creditMetricEvent}
          onChange={(e) => update({ creditMetricEvent: e.target.value })}
          placeholder="e.g., click"
          className="mt-1 block w-full rounded-md border border-gray-300 px-3 py-2 text-sm shadow-sm focus:border-indigo-500 focus:ring-indigo-500"
        />
      </div>

      <div>
        <label htmlFor="max-list-size" className="block text-sm font-medium text-gray-700">
          Max List Size
        </label>
        <input
          id="max-list-size"
          type="number"
          min={1}
          value={config.maxListSize}
          onChange={(e) => update({ maxListSize: parseInt(e.target.value) || 1 })}
          className="mt-1 block w-32 rounded-md border border-gray-300 px-3 py-2 text-sm shadow-sm focus:border-indigo-500 focus:ring-indigo-500"
        />
      </div>
    </div>
  );
}
