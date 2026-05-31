'use client';

import { useReducer, useEffect, useState } from 'react';
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
import { MetricqlSection } from '@/components/metrics/metricql-section';
import { MetricFormPreview } from '@/components/metrics/metric-form-preview';
import { createMetricDefinition, marshalMetricDefinition, listMetricDefinitions } from '@/lib/api';
import { useToast } from '@/lib/toast-context';
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

// ADR-026 Phase 3 / Task D2 (Lock L5). The M5 server emits the
// x-kaizen-deprecation trailer on CUSTOM creates; the UI surfaces it as
// a toast. Detail-page banner deferred (no metric detail page exists yet).
//
// The user-visible string is locked by L5 so operator-facing messaging stays
// consistent across M5 (trailer), the UI toast, and the migration runbook
// referenced at the end. If you change this, also update the runbook anchor
// `docs/runbooks/m5-metric-definitions.md#custom-deprecation`.
export const DEPRECATION_TOAST_MESSAGE =
  'Custom SQL metrics are deprecated. Use MetricQL or structured types instead. See docs/runbooks/m5-metric-definitions.md#custom-deprecation.';

/**
 * ADR-026 Phase 3 / Task D2. Returns true when the just-created metric is
 * a CUSTOM metric and the UI should surface the deprecation toast.
 *
 * Extracted so the type-gate is unit-testable in isolation — the integration
 * test exercises the full Create → router push → toast emission path via the
 * page, and this helper covers the per-type decision matrix (CUSTOM yes;
 * MEAN, FILTERED_MEAN, METRICQL, etc. no).
 */
export function shouldShowCustomDeprecationToast(metric: { type: MetricType }): boolean {
  return metric.type === 'CUSTOM';
}

interface MetricFormState extends MetricFormShellState {
  type: MetricType;
  // Per-type configs: only the one matching `type` is read by the marshaller.
  // Keeping them as separate optional fields avoids reducer-action explosion
  // (see ADR-026 Phase 1 plan "Risks + mitigations").
  filteredMean?: FilteredMeanConfig;
  composite?: CompositeConfig;
  windowedCount?: WindowedCountConfig;
  // ADR-026 Phase 2: MetricQL expression string for the METRICQL type.
  metricqlExpression: string;
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
  metricqlExpression: '',
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
        metricqlExpression: '',
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
      return { ...base, metricqlExpression: state.metricqlExpression };
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
      return state.metricqlExpression.trim().length > 0;
    default:
      return true;
  }
}

/**
 * Experiment ID is not available on the metric creation form (a metric is not
 * yet bound to an experiment at creation time). The MetricQL validator (B4)
 * and preview RPC (B5/C2) accept an empty string — the server resolves
 * @metric_ref existence from the global metric catalog, not per-experiment.
 * A follow-up spec can thread a real experimentId here once the form-shell
 * supports experiment context.
 */
const METRICQL_FORM_EXPERIMENT_ID = '';

export default function NewMetricPage() {
  const router = useRouter();
  const { addToast } = useToast();
  const [state, dispatch] = useReducer(reducer, initialState);

  // Fetch the metric catalog so MetricqlSection can power @metric_ref autocomplete.
  // We need only the IDs — metric names and configs are not required here.
  // This matches the pattern used by OperandPicker (composite-section.tsx) which
  // also calls listMetricDefinitions and maps to IDs.
  const [knownMetricIds, setKnownMetricIds] = useState<string[]>([]);
  useEffect(() => {
    listMetricDefinitions()
      .then((resp) => setKnownMetricIds(resp.metrics.map((m) => m.metricId)))
      .catch(() => {
        // Non-fatal: autocomplete degrades gracefully to empty when the catalog
        // cannot be fetched. The user can still type expressions manually.
      });
  }, []);

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
      // ADR-026 Phase 3 / Task D2 (L5). Decide off the server-echoed `type`
      // rather than the form state — the server is the source of truth and
      // also avoids surfacing a toast if M5 coerced the type for any reason.
      // Toast is queued BEFORE router.push so the persistent ToastProvider
      // (app/layout.tsx) holds it across the navigation.
      if (shouldShowCustomDeprecationToast(created)) {
        addToast(DEPRECATION_TOAST_MESSAGE, 'warning');
      }
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
              <MetricqlSection
                value={state.metricqlExpression}
                onChange={(next) => dispatch({ type: 'SET_METRICQL_EXPRESSION', value: next })}
                experimentId={METRICQL_FORM_EXPERIMENT_ID}
                knownMetricIds={knownMetricIds}
                disabled={state.submitting}
              />
            )}
            {state.type !== 'FILTERED_MEAN'
              && state.type !== 'COMPOSITE'
              && state.type !== 'WINDOWED_COUNT'
              && state.type !== 'METRICQL' && (
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
