'use client';

import type { MetricType } from '@/lib/types';

interface MetricTypeSelectProps {
  value: MetricType;
  onChange: (next: MetricType) => void;
  disabled?: boolean;
}

// Order: legacy types first (most-used), then new ADR-026 Phase 1 types.
// Labels match METRIC_TYPE_BADGE / ALL_METRIC_TYPES in src/app/metrics/page.tsx.
const TYPE_OPTIONS: { value: MetricType; label: string; description: string }[] = [
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

export function MetricTypeSelect({ value, onChange, disabled }: MetricTypeSelectProps) {
  const description = TYPE_OPTIONS.find((o) => o.value === value)?.description ?? '';
  const isDeprecated = value === 'CUSTOM';

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
        {TYPE_OPTIONS.map((opt) => (
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
