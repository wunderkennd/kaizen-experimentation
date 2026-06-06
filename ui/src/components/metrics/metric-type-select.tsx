'use client';

import type { MetricType } from '@/lib/types';

interface MetricTypeSelectProps {
  value: MetricType;
  onChange: (next: MetricType) => void;
  disabled?: boolean;
}

// Order: legacy types first (most-used), then new ADR-026 Phase 1 types.
// Labels match METRIC_TYPE_BADGE / ALL_METRIC_TYPES in src/app/metrics/page.tsx.
const ALL_TYPE_OPTIONS: { value: MetricType; label: string; description: string }[] = [
  { value: 'MEAN',           label: 'Mean',           description: 'Average of a numeric value per user (e.g. watch time).' },
  { value: 'PROPORTION',     label: 'Proportion',     description: 'Binary event rate per user (e.g. conversion).' },
  { value: 'RATIO',          label: 'Ratio',          description: 'Numerator sum / denominator sum, delta-method variance.' },
  { value: 'COUNT',          label: 'Count',          description: 'Event count per user.' },
  { value: 'PERCENTILE',     label: 'Percentile',     description: 'P-th percentile of a distribution.' },
  { value: 'CUSTOM',         label: 'Custom SQL (deprecated)', description: 'Deprecated — use MetricQL or structured types. Existing CUSTOMs continue to work. See migration guide.' },
  { value: 'FILTERED_MEAN',  label: 'Filtered Mean',  description: 'Mean over rows matching a filter (ADR-026 Phase 1).' },
  { value: 'COMPOSITE',      label: 'Composite',      description: 'Combine other metrics with an operator (ADR-026 Phase 1).' },
  { value: 'WINDOWED_COUNT', label: 'Windowed Count', description: 'Event count within N hours of exposure (ADR-026 Phase 1).' },
  { value: 'METRICQL',      label: 'MetricQL expression', description: 'Custom expression with arithmetic, filters, and @metric_ref references (ADR-026 Phase 2).' },
];

// ADR-026 Phase 3 — L6 phase 3.B sunset gate.
//
// Flag key (canonical): `m6.metric_type.custom.hidden` (default: false).
// Today the gate is read from a Next.js public env var so operators can flip
// it at deploy time without depending on an M7 UI flag client:
//
//   NEXT_PUBLIC_METRIC_TYPE_CUSTOM_HIDDEN=true
//
// When this repo grows a first-class M7 UI flag client, replace the env-var
// read below with the equivalent `useFeatureFlag('m6.metric_type.custom.hidden')`
// call — the option-filter logic stays the same. The flag key above is the
// stable contract.
//
// Flipping the gate begins the 2-cycle countdown for #602 (proto enum removal).
// The 4-week zero-CUSTOMs criterion is observed via the
// `metric_definition_custom_created_total` counter emitted from M5 (see
// crates/experimentation-management/src/grpc.rs::create_metric_definition).
//
// Locked plan: docs/superpowers/plans/2026-05-30-adr-026-phase-3-custom-migration.md (L6).
function isCustomHidden(): boolean {
  return process.env.NEXT_PUBLIC_METRIC_TYPE_CUSTOM_HIDDEN === 'true';
}

export function MetricTypeSelect({ value, onChange, disabled }: MetricTypeSelectProps) {
  const options = isCustomHidden()
    ? ALL_TYPE_OPTIONS.filter((o) => o.value !== 'CUSTOM')
    : ALL_TYPE_OPTIONS;
  const description = options.find((o) => o.value === value)?.description ?? '';
  // `isDeprecated` only renders the deprecation icon for CUSTOM when CUSTOM is
  // still in `options`. The current caller (`/metrics/new`) cannot reach the
  // `value='CUSTOM' && isCustomHidden()` state — the option is filtered out so
  // the select can't be set to it — but a future edit-context caller that
  // re-mounts this component on an existing CUSTOM metric would: without the
  // gate, the icon would render next to a `<select>` value that has no
  // matching `<option>`. (Devin PR #603 📝 future-caller defensive gate.)
  const isDeprecated = value === 'CUSTOM' && !isCustomHidden();

  return (
    <div>
      <label htmlFor="metric-type-select" className="block text-sm font-medium text-gray-700">
        Metric Type <span className="text-red-500">*</span>
      </label>
      <select
        id="metric-type-select"
        value={value}
        onChange={(e) => onChange(e.target.value as MetricType)}
        disabled={disabled}
        aria-required="true"
        data-testid="metric-type-select"
        className="mt-1 block w-full rounded-md border border-gray-300 px-3 py-2 text-sm shadow-sm focus:border-indigo-500 focus:ring-indigo-500 disabled:bg-gray-100 disabled:text-gray-500"
      >
        {options.map((opt) => (
          <option key={opt.value} value={opt.value}>{opt.label}</option>
        ))}
      </select>
      <p className="mt-1 text-xs text-gray-500" data-testid="metric-type-description">
        {isDeprecated && (
          <span data-testid="metric-type-deprecated-icon" className="mr-1">
            <svg
              width="14"
              height="14"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              strokeWidth="2"
              strokeLinecap="round"
              strokeLinejoin="round"
              className="inline-block text-amber-500 align-text-bottom"
              aria-hidden="true"
            >
              <path d="m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3Z" />
              <line x1="12" y1="9" x2="12" y2="13" />
              <line x1="12" y1="17" x2="12.01" y2="17" />
            </svg>
            <span className="sr-only">Deprecated metric type</span>
          </span>
        )}
        {description}
      </p>
    </div>
  );
}
