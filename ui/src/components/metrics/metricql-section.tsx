'use client';

/**
 * MetricQL form section (B6, ADR-026 Phase 2 #436).
 *
 * Composes MetricqlEditor (B2) + MetricqlPreview (B5) into the single section
 * rendered inside the metric creation form when the operator selects METRICQL.
 *
 * Import pattern: MetricqlEditor is imported from './metricql' (the lazy-load
 * boundary in index.tsx), never from './metricql/editor' directly.  This keeps
 * the CodeMirror + Lezer bundle out of the initial page load.
 */

import { MetricqlEditor } from './metricql';
import { MetricqlPreview } from './metricql/preview';

export interface MetricqlSectionProps {
  /** The current MetricQL expression value (drives the form's metricqlExpression field). */
  value: string;
  onChange: (next: string) => void;
  /**
   * Experiment ID for the validator (B4) + preview (B5/C2) RPCs.
   *
   * Omit (or pass `null` / `undefined`) when the section is rendered outside an
   * experiment binding — e.g. the metric creation form. M5's ValidateMetricql
   * handler treats an empty experiment_id as global scope and builds the known
   * metric set from the full catalog (Issue #571 Task 1).
   */
  experimentId?: string | null;
  /**
   * Known metric IDs from the form's cached ListMetricDefinitions response.
   * Powers the @-autocomplete (B3). Optimistic cache updates (just-created
   * metrics) are visible without restarting the editor because the editor holds
   * a stable ref to this array (per B2's knownMetricIdsRef pattern).
   */
  knownMetricIds: string[];
  disabled?: boolean;
}

export function MetricqlSection({
  value,
  onChange,
  experimentId,
  knownMetricIds,
  disabled,
}: MetricqlSectionProps) {
  // The preview pane suppresses its fetch when the expression is empty.
  // hasErrors feeds the preview guard — an empty/whitespace expression is the
  // only obvious client-side signal we have without coupling to the linter's
  // internal state. The preview pane also surfaces server diagnostics independently
  // (B5 handles this). For v1, empty = no preview; linter errors surface inline
  // in the editor (B4). This is the accepted design per the task spec.
  const hasObviousErrors = !value.trim();

  return (
    <fieldset disabled={disabled} className="flex flex-col gap-4 rounded border border-violet-200 bg-violet-50/30 p-4" data-testid="metricql-section">
      <legend className="px-2 text-sm font-semibold text-violet-900">METRICQL</legend>

      <div>
        <label className="block text-sm font-medium text-gray-700 mb-1">
          MetricQL expression <span className="text-red-500">*</span>
        </label>
        <MetricqlEditor
          value={value}
          onChange={onChange}
          experimentId={experimentId}
          knownMetricIds={knownMetricIds}
          ariaLabel="MetricQL expression"
          disabled={disabled}
        />
        <p className="mt-1 text-xs text-gray-500">
          Use <code className="font-mono">@metric_id</code> to reference other metrics.
          Type <kbd className="rounded border border-gray-300 px-1 font-mono text-xs">@</kbd> for autocomplete.
          Diagnostics appear inline as you type.
        </p>
      </div>

      <MetricqlPreview
        // Pass null/undefined through directly — MetricqlPreview normalises
        // to '' once at the RPC call site (Issue #597). Mirrors the
        // translation-at-boundaries pattern PR #595 established for the linter.
        experimentId={experimentId}
        metricqlExpression={value}
        hasErrors={hasObviousErrors}
        className="mt-2"
      />
    </fieldset>
  );
}
