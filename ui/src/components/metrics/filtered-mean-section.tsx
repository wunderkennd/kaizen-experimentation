'use client';

import type { FilteredMeanConfig } from '@/lib/types';
import { validateFilteredMeanConfig } from '@/lib/validation';
import { useDebouncedValidation } from '@/hooks/use-debounced-validation';
import { SqlEditor } from './sql-editor';

interface FilteredMeanSectionProps {
  value: FilteredMeanConfig | undefined;
  onChange: (next: FilteredMeanConfig) => void;
  disabled?: boolean;
}

const DEFAULT_CONFIG: FilteredMeanConfig = { filterSql: '', valueColumn: '' };

const INPUT_CLASS =
  'mt-1 block w-full rounded-md border border-gray-300 px-3 py-2 text-sm shadow-sm focus:border-indigo-500 focus:ring-indigo-500 disabled:bg-gray-100 disabled:text-gray-500';

/**
 * FILTERED_MEAN per-type form section (ADR-026 Phase 1).
 *
 * Renders the `value_column` text input and the `filter_sql` CodeMirror editor.
 * Debounced client-side validation surfaces inline below the fieldset; deep
 * server-side checks (SQL allowlist parsing) happen at submit time in B4.
 */
export function FilteredMeanSection({ value, onChange, disabled }: FilteredMeanSectionProps) {
  const cfg = value ?? DEFAULT_CONFIG;
  const validation = useDebouncedValidation(cfg, validateFilteredMeanConfig);

  return (
    <fieldset
      disabled={disabled}
      className="flex flex-col gap-4 rounded-md border border-indigo-200 bg-indigo-50/30 p-4"
      data-testid="filtered-mean-section"
    >
      <legend className="px-2 text-sm font-semibold text-indigo-900">FILTERED_MEAN</legend>

      <div>
        <label htmlFor="filtered-mean-value-column" className="block text-sm font-medium text-gray-700">
          Value column <span className="text-red-500">*</span>
        </label>
        <input
          id="filtered-mean-value-column"
          type="text"
          value={cfg.valueColumn}
          onChange={(e) => onChange({ ...cfg, valueColumn: e.target.value })}
          placeholder="duration_ms"
          aria-required="true"
          aria-describedby="filtered-mean-value-column-hint"
          data-testid="filtered-mean-value-column"
          className={`${INPUT_CLASS} font-mono`}
        />
        <p id="filtered-mean-value-column-hint" className="mt-1 text-xs text-gray-500">
          Lowercase identifier — the numeric column averaged over filtered rows.
        </p>
      </div>

      <div>
        <label htmlFor="filtered-mean-filter-sql" className="block text-sm font-medium text-gray-700">
          Filter SQL <span className="text-red-500">*</span>
        </label>
        <div id="filtered-mean-filter-sql" className="mt-1">
          <SqlEditor
            value={cfg.filterSql}
            onChange={(next) => onChange({ ...cfg, filterSql: next })}
            placeholder="platform = 'mobile' AND duration_ms > 5000"
            ariaLabel="FILTERED_MEAN filter SQL predicate"
            disabled={disabled}
          />
        </div>
        <p className="mt-1 text-xs text-gray-500">
          WHERE-clause predicate. Allowed operators: <code>=</code>, <code>!=</code>,
          {' '}<code>&lt;</code>, <code>&lt;=</code>, <code>&gt;</code>, <code>&gt;=</code>,
          {' '}<code>AND</code>, <code>OR</code>, <code>NOT</code>, <code>IN</code>,
          {' '}<code>IS NULL</code>, <code>IS NOT NULL</code>. No <code>LIKE</code>,
          {' '}<code>BETWEEN</code>, function calls, or subqueries.
        </p>
        <p className="mt-1 text-xs text-amber-700">
          FILTERED_MEAN without a filter? Use <strong>METRIC_TYPE_MEAN</strong> instead — it&apos;s the same computation with one fewer step.
        </p>
      </div>

      {validation.status === 'invalid' && validation.error && (
        <p
          className="text-sm text-red-700"
          role="alert"
          data-testid="filtered-mean-validation-error"
        >
          {validation.error}
        </p>
      )}
    </fieldset>
  );
}
