'use client';

import { useWizard } from '../wizard-context';
import type { GuardrailAction, SequentialMethod } from '@/lib/types';

const SEQUENTIAL_METHODS: SequentialMethod[] = ['MSPRT', 'GST_OBF', 'GST_POCOCK'];

export function MetricsStep() {
  const { state, dispatch } = useWizard();

  const setField = (field: string, value: unknown) =>
    dispatch({ type: 'SET_FIELD', field, value });

  return (
    <section className="space-y-8">
      {/* Metrics */}
      <div>
        <h2 className="mb-4 text-lg font-semibold text-gray-900">Metrics</h2>
        <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
          <div>
            <label htmlFor="exp-primary-metric" className="block text-sm font-medium text-gray-700">
              Primary Metric <span className="text-red-500">*</span>
            </label>
            <input
              id="exp-primary-metric"
              type="text"
              value={state.primaryMetricId}
              onChange={(e) => setField('primaryMetricId', e.target.value)}
              placeholder="e.g., click_through_rate"
              aria-required="true"
              className="mt-1 block w-full rounded-md border border-gray-300 px-3 py-2 text-sm shadow-sm focus:border-indigo-500 focus:ring-indigo-500"
            />
          </div>
          <div>
            <label htmlFor="exp-secondary-metrics" className="block text-sm font-medium text-gray-700">
              Secondary Metrics
            </label>
            <input
              id="exp-secondary-metrics"
              type="text"
              value={state.secondaryMetricsInput}
              onChange={(e) => setField('secondaryMetricsInput', e.target.value)}
              placeholder="Comma-separated, e.g., watch_time, revenue"
              className="mt-1 block w-full rounded-md border border-gray-300 px-3 py-2 text-sm shadow-sm focus:border-indigo-500 focus:ring-indigo-500"
            />
          </div>
        </div>
      </div>

      {/* Guardrails */}
      <div>
        <div className="mb-4 flex items-center justify-between">
          <h2 className="text-lg font-semibold text-gray-900">Guardrails</h2>
          <button
            type="button"
            onClick={() => dispatch({ type: 'ADD_GUARDRAIL' })}
            className="rounded-md border border-gray-300 bg-white px-3 py-1.5 text-sm font-medium text-gray-700 hover:bg-gray-50"
          >
            Add Guardrail
          </button>
        </div>
        {state.guardrails.length > 0 ? (
          <div className="space-y-3">
            {state.guardrails.map((g, i) => (
              <div key={i} className="flex items-end gap-3 rounded-lg border border-gray-200 bg-white p-3">
                <div className="flex-1">
                  <label className="block text-xs font-medium text-gray-500">Metric ID</label>
                  <input
                    type="text"
                    value={g.metricId}
                    onChange={(e) => dispatch({ type: 'UPDATE_GUARDRAIL', index: i, field: 'metricId', value: e.target.value })}
                    aria-label={`Guardrail ${i + 1} metric`}
                    className="mt-1 block w-full rounded border border-gray-300 px-2 py-1 text-sm"
                  />
                </div>
                <div className="w-28">
                  <label className="block text-xs font-medium text-gray-500">Threshold</label>
                  <input
                    type="number"
                    step="any"
                    value={g.threshold}
                    onChange={(e) => dispatch({ type: 'UPDATE_GUARDRAIL', index: i, field: 'threshold', value: parseFloat(e.target.value) || 0 })}
                    aria-label={`Guardrail ${i + 1} threshold`}
                    className="mt-1 block w-full rounded border border-gray-300 px-2 py-1 text-sm"
                  />
                </div>
                <div className="w-28">
                  <label className="block text-xs font-medium text-gray-500">Breaches</label>
                  <input
                    type="number"
                    min={1}
                    value={g.consecutiveBreachesRequired}
                    onChange={(e) => dispatch({ type: 'UPDATE_GUARDRAIL', index: i, field: 'consecutiveBreachesRequired', value: parseInt(e.target.value) || 1 })}
                    aria-label={`Guardrail ${i + 1} breaches required`}
                    className="mt-1 block w-full rounded border border-gray-300 px-2 py-1 text-sm"
                  />
                </div>
                <button
                  type="button"
                  onClick={() => dispatch({ type: 'REMOVE_GUARDRAIL', index: i })}
                  className="mb-1 text-sm text-red-600 hover:text-red-800"
                >
                  Remove
                </button>
              </div>
            ))}
            <div>
              <label htmlFor="guardrail-action" className="block text-sm font-medium text-gray-700">Action on Breach</label>
              <select
                id="guardrail-action"
                value={state.guardrailAction}
                onChange={(e) => setField('guardrailAction', e.target.value as GuardrailAction)}
                className="mt-1 block w-48 rounded-md border border-gray-300 px-3 py-2 text-sm shadow-sm"
              >
                <option value="AUTO_PAUSE">Auto-Pause</option>
                <option value="ALERT_ONLY">Alert Only</option>
              </select>
            </div>
          </div>
        ) : (
          <p className="text-sm text-gray-500">No guardrails configured. Click &ldquo;Add Guardrail&rdquo; to add one.</p>
        )}
      </div>

      {/* Sequential Testing */}
      <div>
        <h2 className="mb-4 text-lg font-semibold text-gray-900">Sequential Testing</h2>
        <label className="flex items-center gap-2">
          <input
            type="checkbox"
            checked={state.enableSequential}
            onChange={(e) => setField('enableSequential', e.target.checked)}
            className="rounded border-gray-300"
          />
          <span className="text-sm text-gray-700">Enable sequential testing</span>
        </label>
        {state.enableSequential && (
          <div className="mt-3 grid grid-cols-1 gap-4 sm:grid-cols-3">
            <div>
              <label htmlFor="seq-method" className="block text-sm font-medium text-gray-700">Method</label>
              <select
                id="seq-method"
                value={state.sequentialMethod}
                onChange={(e) => setField('sequentialMethod', e.target.value as SequentialMethod)}
                className="mt-1 block w-full rounded-md border border-gray-300 px-3 py-2 text-sm shadow-sm"
              >
                {SEQUENTIAL_METHODS.map((m) => (
                  <option key={m} value={m}>{m}</option>
                ))}
              </select>
            </div>
            <div>
              <label htmlFor="seq-looks" className="block text-sm font-medium text-gray-700">Planned Looks</label>
              <input
                id="seq-looks"
                type="number"
                min={0}
                value={state.plannedLooks}
                onChange={(e) => setField('plannedLooks', parseInt(e.target.value) || 0)}
                className="mt-1 block w-full rounded-md border border-gray-300 px-3 py-2 text-sm shadow-sm"
              />
            </div>
            <div>
              <label htmlFor="seq-alpha" className="block text-sm font-medium text-gray-700">Overall Alpha</label>
              <input
                id="seq-alpha"
                type="number"
                min={0}
                max={1}
                step={0.01}
                value={state.overallAlpha}
                onChange={(e) => setField('overallAlpha', parseFloat(e.target.value) || 0.05)}
                className="mt-1 block w-full rounded-md border border-gray-300 px-3 py-2 text-sm shadow-sm"
              />
            </div>
          </div>
        )}
      </div>
    </section>
  );
}
