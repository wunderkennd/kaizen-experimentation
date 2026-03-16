'use client';

import { useWizard } from '../wizard-context';
import type { BanditAlgorithm } from '@/lib/types';

const ALGORITHMS: { value: BanditAlgorithm; label: string }[] = [
  { value: 'THOMPSON_SAMPLING', label: 'Thompson Sampling' },
  { value: 'LINEAR_UCB', label: 'Linear UCB' },
  { value: 'THOMPSON_LINEAR', label: 'Thompson Linear' },
  { value: 'NEURAL_CONTEXTUAL', label: 'Neural Contextual' },
];

export function BanditConfigForm() {
  const { state, dispatch } = useWizard();
  const config = state.banditExperimentConfig;
  const isContextual = state.type === 'CONTEXTUAL_BANDIT';

  const update = (partial: Partial<typeof config>) =>
    dispatch({ type: 'SET_FIELD', field: 'banditExperimentConfig', value: { ...config, ...partial } });

  const updateFeatureKey = (index: number, value: string) => {
    const keys = [...config.contextFeatureKeys];
    keys[index] = value;
    update({ contextFeatureKeys: keys });
  };

  const addFeatureKey = () => update({ contextFeatureKeys: [...config.contextFeatureKeys, ''] });

  const removeFeatureKey = (index: number) =>
    update({ contextFeatureKeys: config.contextFeatureKeys.filter((_, i) => i !== index) });

  return (
    <div className="space-y-4">
      <div>
        <label htmlFor="bandit-algorithm" className="block text-sm font-medium text-gray-700">
          Algorithm <span className="text-red-500">*</span>
        </label>
        <select
          id="bandit-algorithm"
          value={config.algorithm}
          onChange={(e) => update({ algorithm: e.target.value as BanditAlgorithm })}
          className="mt-1 block w-full rounded-md border border-gray-300 px-3 py-2 text-sm shadow-sm focus:border-indigo-500 focus:ring-indigo-500"
        >
          {ALGORITHMS.map((a) => (
            <option key={a.value} value={a.value}>{a.label}</option>
          ))}
        </select>
      </div>

      <div>
        <label htmlFor="reward-metric" className="block text-sm font-medium text-gray-700">
          Reward Metric ID <span className="text-red-500">*</span>
        </label>
        <input
          id="reward-metric"
          type="text"
          value={config.rewardMetricId}
          onChange={(e) => update({ rewardMetricId: e.target.value })}
          placeholder="e.g., conversion_rate"
          className="mt-1 block w-full rounded-md border border-gray-300 px-3 py-2 text-sm shadow-sm focus:border-indigo-500 focus:ring-indigo-500"
        />
      </div>

      {isContextual && (
        <div>
          <div className="mb-2 flex items-center justify-between">
            <label className="block text-sm font-medium text-gray-700">
              Context Feature Keys <span className="text-red-500">*</span>
            </label>
            <button
              type="button"
              onClick={addFeatureKey}
              className="rounded-md border border-gray-300 bg-white px-2 py-1 text-xs font-medium text-gray-700 hover:bg-gray-50"
            >
              Add Feature
            </button>
          </div>
          {config.contextFeatureKeys.length === 0 && (
            <p className="text-sm text-gray-500">No context features configured. Click &ldquo;Add Feature&rdquo; to add one.</p>
          )}
          {config.contextFeatureKeys.map((key, i) => (
            <div key={i} className="mb-2 flex gap-2">
              <input
                type="text"
                value={key}
                onChange={(e) => updateFeatureKey(i, e.target.value)}
                placeholder={`Feature key ${i + 1}`}
                aria-label={`Context feature ${i + 1}`}
                className="block w-full rounded-md border border-gray-300 px-3 py-2 text-sm shadow-sm focus:border-indigo-500 focus:ring-indigo-500"
              />
              <button
                type="button"
                onClick={() => removeFeatureKey(i)}
                className="text-sm text-red-600 hover:text-red-800"
              >
                Remove
              </button>
            </div>
          ))}
        </div>
      )}

      <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
        <div>
          <label htmlFor="exploration-fraction" className="block text-sm font-medium text-gray-700">
            Min Exploration Fraction
          </label>
          <input
            id="exploration-fraction"
            type="number"
            min={0}
            max={1}
            step={0.01}
            value={config.minExplorationFraction}
            onChange={(e) => update({ minExplorationFraction: parseFloat(e.target.value) || 0 })}
            className="mt-1 block w-full rounded-md border border-gray-300 px-3 py-2 text-sm shadow-sm focus:border-indigo-500 focus:ring-indigo-500"
          />
        </div>
        <div>
          <label htmlFor="warmup-obs" className="block text-sm font-medium text-gray-700">
            Warmup Observations
          </label>
          <input
            id="warmup-obs"
            type="number"
            min={0}
            value={config.warmupObservations}
            onChange={(e) => update({ warmupObservations: parseInt(e.target.value) || 0 })}
            className="mt-1 block w-full rounded-md border border-gray-300 px-3 py-2 text-sm shadow-sm focus:border-indigo-500 focus:ring-indigo-500"
          />
        </div>
      </div>
    </div>
  );
}
