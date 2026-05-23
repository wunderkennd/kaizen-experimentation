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
import { CompositeSection } from '@/components/metrics/composite-section';
import { WindowedCountSection } from '@/components/metrics/windowed-count-section';
import { MetricqlEditor } from '@/components/editors/metricql-editor';
import { MetricFormPreview } from '@/components/metrics/metric-form-preview';
import { createMetricDefinition, marshalMetricDefinition } from '@/lib/api';
import {
  validateFilteredMeanConfig,
  validateCompositeConfig,
  validateWindowedCountConfig,
} from '@/lib/validation';
import type {
  MetricType,
  MetricDefinition,
  FilteredMeanConfig,
  CompositeConfig,
  WindowedCountConfig,
} from '@/lib/types';

interface MetricFormState extends MetricFormShellState {
  type: MetricType;
  // Per-type configs: only the one matching `type` is read by the marshaller.
  // Keeping them as separate optional fields avoids reducer-action explosion
  // (see ADR-026 Phase 1 plan "Risks + mitigations").
  filteredMean?: FilteredMeanConfig;
  composite?: CompositeConfig;
  windowedCount?: WindowedCountConfig;
  metricqlExpression?: string;
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
  | { type: 'SET_METRICQL_EXPRESSION'; value: string }
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
      // type cannot leak into the marshalled submit payload.
      return {
        ...state,
        type: action.value,
        filteredMean: undefined,
        composite: undefined,
        windowedCount: undefined,
        metricqlExpression: undefined,
      };
    case 'SET_FILTERED_MEAN':
      return { ...state, filteredMean: action.value };
    case 'SET_COMPOSITE':
      return { ...state, composite: action.value };
    case 'SET_WINDOWED_COUNT':
      return { ...state, windowedCount: action.value };
    case 'SET_METRICQL_EXPRESSION':
      return { ...state, metricqlExpression: action.value };
    case 'SET_SUBMITTING':
      return { ...state, submitting: action.value };
    case 'SET_SERVER_ERROR':
      return { ...state, serverError: action.value };
  }
}

/**
 * Marshal the form state into the wire shape of `MetricDefinition`.
 *
 * The reducer keeps the 3 per-type configs as separate optional fields; this
 * function reads only the one that matches `state.type`. Legacy 6 types
 * leave `typeConfig` undefined (the server validates flat fields like
 * `sourceEventType` / `numeratorEventType` / `percentile`, which the Phase 1
 * UI doesn't author — adopting the legacy types in this form is out of scope
 * for ADR-026 Phase 1 per `docs/superpowers/plans/2026-05-17-adr-026-phase-1-m6-ui.md`).
 */
function buildMetricFromState(state: MetricFormState): MetricDefinition {
  const base: MetricDefinition = {
    metricId: state.metricId,
    name: state.name,
    description: state.description,
    type: state.type,
    // The form shell does not yet author `sourceEventType` / `isQoeMetric`
    // (those belong to the legacy 6 types which Phase 1 leaves alone).
    // Pass empty/false; the server applies its defaults and echoes back.
    sourceEventType: '',
    lowerIsBetter: state.lowerIsBetter,
    isQoeMetric: false,
    // ADR-014 multi-stakeholder fields. Collected by MetricFormShell — must
    // round-trip to the server or every metric gets categorized UNSPECIFIED
    // (Devin BUG-0001 on PR #555).
    stakeholder: state.stakeholder,
    aggregationLevel: state.aggregationLevel,
  };

  switch (state.type) {
    case 'FILTERED_MEAN':
      if (state.filteredMean) {
        return { ...base, typeConfig: { case: 'filteredMean', value: state.filteredMean } };
      }
      return base;
    case 'COMPOSITE':
      if (state.composite) {
        return { ...base, typeConfig: { case: 'composite', value: state.composite } };
      }
      return base;
    case 'WINDOWED_COUNT':
      if (state.windowedCount) {
        return { ...base, typeConfig: { case: 'windowedCount', value: state.windowedCount } };
      }
      return base;
    case 'METRICQL':
      if (state.metricqlExpression !== undefined) {
        return { ...base, metricqlExpression: state.metricqlExpression };
      }
      return base;
    default:
      return base;
  }
}

/**
 * Whole-form validity gate for the submit button. Common fields must be
 * non-empty and the per-type config (if any) must pass its inline validator.
 * Legacy 6 types have no client-side config to validate — the server gates.
 */
function isFormValid(state: MetricFormState): boolean {
  if (!state.metricId || state.metricId.trim().length === 0) return false;
  if (!state.name || state.name.trim().length === 0) return false;

  switch (state.type) {
    case 'FILTERED_MEAN':
      return !!state.filteredMean && validateFilteredMeanConfig(state.filteredMean).valid;
    case 'COMPOSITE':
      return !!state.composite && validateCompositeConfig(state.composite).valid;
    case 'WINDOWED_COUNT':
      return !!state.windowedCount && validateWindowedCountConfig(state.windowedCount).valid;
    case 'METRICQL':
      return !!state.metricqlExpression && state.metricqlExpression.trim().length > 0;
    default:
      return true;
  }
}

export default function NewMetricPage() {
  const router = useRouter();
  const [state, dispatch] = useReducer(reducer, initialState);

  const handleCancel = () => {
    router.push('/metrics');
  };

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    dispatch({ type: 'SET_SUBMITTING', value: true });
    dispatch({ type: 'SET_SERVER_ERROR', value: undefined });

    const metric = buildMetricFromState(state);

    try {
      const created = await createMetricDefinition(metric);
      router.push(`/metrics?created=${encodeURIComponent(created.metricId)}`);
    } catch (err) {
      const message = err instanceof Error ? err.message : 'Submission failed';
      dispatch({ type: 'SET_SERVER_ERROR', value: message });
      dispatch({ type: 'SET_SUBMITTING', value: false });
    }
  }

  const previewMetric = buildMetricFromState(state);
  const formValid = isFormValid(state);

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
              <CompositeSection
                value={state.composite}
                onChange={(next) => dispatch({ type: 'SET_COMPOSITE', value: next })}
                disabled={state.submitting}
              />
            )}
            {state.type === 'WINDOWED_COUNT' && (
              <WindowedCountSection
                value={state.windowedCount}
                onChange={(next) => dispatch({ type: 'SET_WINDOWED_COUNT', value: next })}
                disabled={state.submitting}
              />
            )}
            {state.type === 'METRICQL' && (
              <MetricqlEditor
                value={state.metricqlExpression || ''}
                onChange={(next) => dispatch({ type: 'SET_METRICQL_EXPRESSION', value: next })}
                disabled={state.submitting}
                metricId={state.metricId}
              />
            )}
            {state.type === 'CUSTOM' && (
              <div className="rounded-md border border-amber-300 bg-amber-50 p-4 text-sm text-amber-800">
                <div className="flex gap-2">
                  <svg className="h-5 w-5 text-amber-600 shrink-0 animate-pulse" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth="2">
                    <path strokeLinecap="round" strokeLinejoin="round" d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z" />
                  </svg>
                  <div>
                    <h4 className="font-semibold text-amber-900 mb-1">Custom SQL Metric Deprecation</h4>
                    <p>
                      CUSTOM raw SQL metrics are deprecated and will be completely removed in a future release.
                      Please choose a Structured metric type (Filtered Mean, Composite, Windowed Count) or use <strong>MetricQL</strong> instead.
                    </p>
                  </div>
                </div>
              </div>
            )}
            {state.type !== 'FILTERED_MEAN'
              && state.type !== 'COMPOSITE'
              && state.type !== 'WINDOWED_COUNT'
              && state.type !== 'METRICQL'
              && state.type !== 'CUSTOM' && (
              <p>
                Type-specific fields for legacy types are out of scope for ADR-026 Phase 1.
                Continue authoring these metric types via the M5 API directly until a follow-up
                spec covers them in the UI.
              </p>
            )}
          </section>

          <section>
            <h2 className="mb-2 text-lg font-semibold text-gray-900">Preview</h2>
            <MetricFormPreview metric={previewMetric} marshal={marshalMetricDefinition} />
          </section>

          {state.serverError && (
            <div
              role="alert"
              className="rounded-md border border-red-300 bg-red-50 p-3 text-sm text-red-700"
              data-testid="metric-server-error"
            >
              <strong>Server rejected:</strong> {state.serverError}
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
              disabled={state.submitting || !formValid}
              className="rounded-md bg-indigo-600 px-4 py-2 text-sm font-medium text-white shadow-sm hover:bg-indigo-700 disabled:cursor-not-allowed disabled:bg-gray-300"
              data-testid="metric-submit-button"
            >
              {state.submitting ? 'Creating…' : 'Create Metric'}
            </button>
          </div>
        </div>
      </form>
    </div>
  );
}
