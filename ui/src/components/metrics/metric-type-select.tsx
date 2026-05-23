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
  { value: 'CUSTOM',         label: 'Custom SQL (Deprecated)', description: 'Arbitrary Spark SQL (DEPRECATED — prefer structured types or MetricQL).' },
  { value: 'FILTERED_MEAN',  label: 'Filtered Mean',  description: 'Mean over rows matching a filter (ADR-026 Phase 1).' },
  { value: 'COMPOSITE',      label: 'Composite',      description: 'Combine other metrics with an operator (ADR-026 Phase 1).' },
  { value: 'WINDOWED_COUNT', label: 'Windowed Count', description: 'Event count within N hours of exposure (ADR-026 Phase 1).' },
  { value: 'METRICQL',       label: 'MetricQL Expression', description: 'Declarative metric expression language with autocomplete and cycle validation.' },
];

export function MetricTypeSelect({ value, onChange, disabled }: MetricTypeSelectProps) {
  const description = TYPE_OPTIONS.find((o) => o.value === value)?.description ?? '';

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
        {description}
      </p>
    </div>
  );
}
