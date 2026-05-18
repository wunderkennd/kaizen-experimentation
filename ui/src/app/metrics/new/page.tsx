'use client';

import { useReducer } from 'react';
import { useRouter } from 'next/navigation';

import { Breadcrumb } from '@/components/breadcrumb';
import {
  MetricFormShell,
  type MetricFormShellState,
  type MetricStakeholder,
  type MetricAggregationLevel,
} from '@/components/metrics/metric-form-shell';
import { MetricTypeSelect } from '@/components/metrics/metric-type-select';
import { FilteredMeanSection } from '@/components/metrics/filtered-mean-section';
import { WindowedCountSection } from '@/components/metrics/windowed-count-section';
import type {
  MetricType,
  FilteredMeanConfig,
  CompositeConfig,
  WindowedCountConfig,
} from '@/lib/types';

interface MetricFormState extends MetricFormShellState {
  type: MetricType;
  // Per-type configs: only the one matching `type` is read by the marshaller
  // in B4. Keeping them as separate optional fields avoids reducer-action
  // explosion (see ADR-026 Phase 1 plan "Risks + mitigations").
  filteredMean?: FilteredMeanConfig;
  composite?: CompositeConfig;
  windowedCount?: WindowedCountConfig;
  // UI-only
  submitting: boolean;
  serverError?: string;
}

type Action =
  | { type: 'SET_FIELD'; key: keyof MetricFormShellState; value: MetricFormShellState[keyof MetricFormShellState] }
  | { type: 'SET_TYPE'; value: MetricType }
  | { type: 'SET_FILTERED_MEAN'; value: FilteredMeanConfig }
  | { type: 'SET_COMPOSITE'; value: CompositeConfig }
  | { type: 'SET_WINDOWED_COUNT'; value: WindowedCountConfig }
  | { type: 'SET_SUBMITTING'; value: boolean }
  | { type: 'SET_SERVER_ERROR'; value: string | undefined };

const initialState: MetricFormState = {
  metricId: '',
  name: '',
  description: '',
  type: 'MEAN',
  stakeholder: 'USER' as MetricStakeholder,
  aggregationLevel: 'USER' as MetricAggregationLevel,
  lowerIsBetter: false,
  submitting: false,
};

function reducer(state: MetricFormState, action: Action): MetricFormState {
  switch (action.type) {
    case 'SET_FIELD':
      return { ...state, [action.key]: action.value };
    case 'SET_TYPE':
      // Clear type-specific configs so stale data from a previously selected
      // type cannot leak into the marshalled submit payload (B4).
      return {
        ...state,
        type: action.value,
        filteredMean: undefined,
        composite: undefined,
        windowedCount: undefined,
      };
    case 'SET_FILTERED_MEAN':
      return { ...state, filteredMean: action.value };
    case 'SET_COMPOSITE':
      return { ...state, composite: action.value };
    case 'SET_WINDOWED_COUNT':
      return { ...state, windowedCount: action.value };
    case 'SET_SUBMITTING':
      return { ...state, submitting: action.value };
    case 'SET_SERVER_ERROR':
      return { ...state, serverError: action.value };
  }
}

export default function NewMetricPage() {
  const router = useRouter();
  const [state, dispatch] = useReducer(reducer, initialState);

  const handleCancel = () => {
    router.push('/metrics');
  };

  // Submit handler is intentionally a no-op in A3 — the per-type sections
  // (B1/B2/B3) and the marshaller (B4) land in follow-up commits. The button
  // is also `disabled` so this branch only runs if a user bypasses the UI.
  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
  };

  return (
    <div>
      <Breadcrumb items={[
        { label: 'Experiments', href: '/' },
        { label: 'Metrics', href: '/metrics' },
        { label: 'New Metric' },
      ]} />

      <h1 className="mb-6 text-2xl font-bold text-gray-900">Create Metric Definition</h1>

      <form
        onSubmit={handleSubmit}
        className="rounded-lg border border-gray-200 bg-white p-6"
        data-testid="metric-create-form"
      >
        <div className="flex flex-col gap-6">
          <section>
            <h2 className="mb-4 text-lg font-semibold text-gray-900">Basics</h2>
            <MetricFormShell
              state={state}
              onChange={(key, value) => dispatch({ type: 'SET_FIELD', key, value })}
              disabled={state.submitting}
            />
          </section>

          <section>
            <h2 className="mb-4 text-lg font-semibold text-gray-900">Type</h2>
            <MetricTypeSelect
              value={state.type}
              onChange={(next) => dispatch({ type: 'SET_TYPE', value: next })}
              disabled={state.submitting}
            />
          </section>

          <section
            data-testid="type-specific-section"
            data-metric-type={state.type}
            className="rounded-md border border-dashed border-gray-300 bg-gray-50 p-4 text-sm text-gray-600"
          >
            {state.type === 'FILTERED_MEAN' && (
              <FilteredMeanSection
                value={state.filteredMean}
                onChange={(next) => dispatch({ type: 'SET_FILTERED_MEAN', value: next })}
                disabled={state.submitting}
              />
            )}
            {state.type === 'COMPOSITE' && (
              <p>
                <code className="font-mono">{'<CompositeSection />'}</code> — coming in B2
                (operator + operand picker).
              </p>
            )}
            {state.type === 'WINDOWED_COUNT' && (
              <WindowedCountSection
                value={state.windowedCount}
                onChange={(next) => dispatch({ type: 'SET_WINDOWED_COUNT', value: next })}
                disabled={state.submitting}
              />
            )}
            {state.type !== 'FILTERED_MEAN'
              && state.type !== 'COMPOSITE'
              && state.type !== 'WINDOWED_COUNT' && (
              <p>
                Type-specific fields for legacy types are out of scope for ADR-026 Phase 1.
                Continue authoring these metric types via the M5 API directly until a follow-up
                spec covers them in the UI.
              </p>
            )}
          </section>

          {state.serverError && (
            <div
              role="alert"
              className="rounded-md border border-red-300 bg-red-50 p-3 text-sm text-red-700"
              data-testid="metric-server-error"
            >
              {state.serverError}
            </div>
          )}

          <div className="flex justify-end gap-2 border-t border-gray-200 pt-4">
            <button
              type="button"
              onClick={handleCancel}
              className="rounded-md border border-gray-300 bg-white px-4 py-2 text-sm font-medium text-gray-700 hover:bg-gray-50"
              data-testid="metric-cancel-button"
            >
              Cancel
            </button>
            <button
              type="submit"
              disabled
              title="Submit wires up in B4 — A3 ships the form shell + type dropdown only."
              className="rounded-md bg-indigo-600 px-4 py-2 text-sm font-medium text-white shadow-sm hover:bg-indigo-700 disabled:cursor-not-allowed disabled:bg-gray-300"
              data-testid="metric-submit-button"
            >
              Create Metric
            </button>
          </div>
        </div>
      </form>
    </div>
  );
}
