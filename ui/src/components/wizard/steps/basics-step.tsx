'use client';

import { useWizard } from '../wizard-context';
import { TYPE_LABELS } from '@/lib/utils';
import type { ExperimentType } from '@/lib/types';

const EXPERIMENT_TYPES: ExperimentType[] = [
  'AB', 'MULTIVARIATE', 'INTERLEAVING', 'SESSION_LEVEL',
  'PLAYBACK_QOE', 'MAB', 'CONTEXTUAL_BANDIT', 'CUMULATIVE_HOLDOUT',
];

export function BasicsStep() {
  const { state, dispatch } = useWizard();

  const setField = (field: string, value: unknown) =>
    dispatch({ type: 'SET_FIELD', field, value });

  return (
    <section>
      <h2 className="mb-4 text-lg font-semibold text-gray-900">Basic Information</h2>
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
        <div>
          <label htmlFor="exp-name" className="block text-sm font-medium text-gray-700">
            Name <span className="text-red-500">*</span>
          </label>
          <input
            id="exp-name"
            type="text"
            value={state.name}
            onChange={(e) => setField('name', e.target.value)}
            placeholder="e.g., homepage_recs_v3"
            aria-required="true"
            className="mt-1 block w-full rounded-md border border-gray-300 px-3 py-2 text-sm shadow-sm focus:border-indigo-500 focus:ring-indigo-500"
          />
        </div>
        <div>
          <label htmlFor="exp-owner" className="block text-sm font-medium text-gray-700">
            Owner Email <span className="text-red-500">*</span>
          </label>
          <input
            id="exp-owner"
            type="email"
            value={state.ownerEmail}
            onChange={(e) => setField('ownerEmail', e.target.value)}
            placeholder="e.g., alice@streamco.com"
            aria-required="true"
            className="mt-1 block w-full rounded-md border border-gray-300 px-3 py-2 text-sm shadow-sm focus:border-indigo-500 focus:ring-indigo-500"
          />
        </div>
        <div className="sm:col-span-2">
          <label htmlFor="exp-description" className="block text-sm font-medium text-gray-700">
            Description
          </label>
          <textarea
            id="exp-description"
            value={state.description}
            onChange={(e) => setField('description', e.target.value)}
            rows={2}
            placeholder="What hypothesis is this experiment testing?"
            className="mt-1 block w-full rounded-md border border-gray-300 px-3 py-2 text-sm shadow-sm focus:border-indigo-500 focus:ring-indigo-500"
          />
        </div>
        <div>
          <label htmlFor="exp-type" className="block text-sm font-medium text-gray-700">
            Experiment Type <span className="text-red-500">*</span>
          </label>
          <select
            id="exp-type"
            value={state.type}
            onChange={(e) => setField('type', e.target.value as ExperimentType)}
            aria-required="true"
            className="mt-1 block w-full rounded-md border border-gray-300 px-3 py-2 text-sm shadow-sm focus:border-indigo-500 focus:ring-indigo-500"
          >
            {EXPERIMENT_TYPES.map((t) => (
              <option key={t} value={t}>{TYPE_LABELS[t]}</option>
            ))}
          </select>
        </div>
        <div>
          <label htmlFor="exp-layer" className="block text-sm font-medium text-gray-700">
            Layer ID <span className="text-red-500">*</span>
          </label>
          <input
            id="exp-layer"
            type="text"
            value={state.layerId}
            onChange={(e) => setField('layerId', e.target.value)}
            placeholder="e.g., layer-homepage"
            aria-required="true"
            className="mt-1 block w-full rounded-md border border-gray-300 px-3 py-2 text-sm shadow-sm focus:border-indigo-500 focus:ring-indigo-500"
          />
        </div>
        <div>
          <label htmlFor="exp-targeting" className="block text-sm font-medium text-gray-700">
            Targeting Rule ID
          </label>
          <input
            id="exp-targeting"
            type="text"
            value={state.targetingRuleId}
            onChange={(e) => setField('targetingRuleId', e.target.value)}
            placeholder="Optional"
            className="mt-1 block w-full rounded-md border border-gray-300 px-3 py-2 text-sm shadow-sm focus:border-indigo-500 focus:ring-indigo-500"
          />
        </div>
        <div className="flex items-end">
          <label className="flex items-center gap-2">
            <input
              type="checkbox"
              checked={state.isCumulativeHoldout}
              onChange={(e) => setField('isCumulativeHoldout', e.target.checked)}
              className="rounded border-gray-300"
            />
            <span className="text-sm text-gray-700">Cumulative holdout experiment</span>
          </label>
        </div>
      </div>
    </section>
  );
}
