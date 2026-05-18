'use client';

/**
 * Common metric-creation fields shared by every metric type (ADR-026 Phase 1).
 *
 * MetricStakeholder / MetricAggregationLevel are defined locally here as string
 * unions because `ui/src/lib/types.ts` deliberately omits them (see the
 * ADR-026 Phase 1 comment at the top of the metrics section in types.ts —
 * the hand-rolled local shape currently mirrors only the subset of
 * MetricDefinition fields the M6 read path uses). The follow-up migration to
 * generated proto-es types (#issue) will replace these with the generated
 * enums; the string values here match the proto enum identifier names
 * (proto/experimentation/common/v1/metric.proto:7-30).
 */

export type MetricStakeholder = 'USER' | 'PROVIDER' | 'PLATFORM';

export type MetricAggregationLevel = 'USER' | 'EXPERIMENT' | 'PROVIDER';

export interface MetricFormShellState {
  metricId: string;
  name: string;
  description: string;
  stakeholder: MetricStakeholder;
  aggregationLevel: MetricAggregationLevel;
  lowerIsBetter: boolean;
}

interface MetricFormShellProps {
  state: MetricFormShellState;
  onChange: <K extends keyof MetricFormShellState>(key: K, value: MetricFormShellState[K]) => void;
  disabled?: boolean;
}

const STAKEHOLDER_OPTIONS: { value: MetricStakeholder; label: string }[] = [
  { value: 'USER',     label: 'User' },
  { value: 'PROVIDER', label: 'Provider' },
  { value: 'PLATFORM', label: 'Platform' },
];

const AGGREGATION_OPTIONS: { value: MetricAggregationLevel; label: string }[] = [
  { value: 'USER',       label: 'User' },
  { value: 'EXPERIMENT', label: 'Experiment' },
  { value: 'PROVIDER',   label: 'Provider' },
];

const INPUT_CLASS =
  'mt-1 block w-full rounded-md border border-gray-300 px-3 py-2 text-sm shadow-sm focus:border-indigo-500 focus:ring-indigo-500 disabled:bg-gray-100 disabled:text-gray-500';

export function MetricFormShell({ state, onChange, disabled }: MetricFormShellProps) {
  return (
    <fieldset disabled={disabled} className="grid grid-cols-1 gap-4 sm:grid-cols-2">
      <div>
        <label htmlFor="metric-id" className="block text-sm font-medium text-gray-700">
          Metric ID <span className="text-red-500">*</span>
        </label>
        <input
          id="metric-id"
          type="text"
          value={state.metricId}
          onChange={(e) => onChange('metricId', e.target.value)}
          placeholder="e.g., mobile_watch_time"
          aria-required="true"
          aria-describedby="metric-id-hint"
          data-testid="metric-id-input"
          className={`${INPUT_CLASS} font-mono`}
        />
        <p id="metric-id-hint" className="mt-1 text-xs text-gray-500">
          Lowercase identifier — letters, digits, underscores. Immutable after creation.
        </p>
      </div>

      <div>
        <label htmlFor="metric-name" className="block text-sm font-medium text-gray-700">
          Display Name <span className="text-red-500">*</span>
        </label>
        <input
          id="metric-name"
          type="text"
          value={state.name}
          onChange={(e) => onChange('name', e.target.value)}
          placeholder="e.g., Mobile Watch Time"
          aria-required="true"
          data-testid="metric-name-input"
          className={INPUT_CLASS}
        />
      </div>

      <div className="sm:col-span-2">
        <label htmlFor="metric-description" className="block text-sm font-medium text-gray-700">
          Description
        </label>
        <textarea
          id="metric-description"
          value={state.description}
          onChange={(e) => onChange('description', e.target.value)}
          rows={2}
          placeholder="What does this metric measure, and when should you use it?"
          data-testid="metric-description-input"
          className={INPUT_CLASS}
        />
      </div>

      <div>
        <label htmlFor="metric-stakeholder" className="block text-sm font-medium text-gray-700">
          Stakeholder <span className="text-red-500">*</span>
        </label>
        <select
          id="metric-stakeholder"
          value={state.stakeholder}
          onChange={(e) => onChange('stakeholder', e.target.value as MetricStakeholder)}
          aria-required="true"
          data-testid="metric-stakeholder-select"
          className={INPUT_CLASS}
        >
          {STAKEHOLDER_OPTIONS.map((opt) => (
            <option key={opt.value} value={opt.value}>{opt.label}</option>
          ))}
        </select>
      </div>

      <div>
        <label htmlFor="metric-aggregation" className="block text-sm font-medium text-gray-700">
          Aggregation Level <span className="text-red-500">*</span>
        </label>
        <select
          id="metric-aggregation"
          value={state.aggregationLevel}
          onChange={(e) => onChange('aggregationLevel', e.target.value as MetricAggregationLevel)}
          aria-required="true"
          data-testid="metric-aggregation-select"
          className={INPUT_CLASS}
        >
          {AGGREGATION_OPTIONS.map((opt) => (
            <option key={opt.value} value={opt.value}>{opt.label}</option>
          ))}
        </select>
      </div>

      <div className="sm:col-span-2 flex items-center">
        <label className="flex items-center gap-2 text-sm text-gray-700">
          <input
            type="checkbox"
            checked={state.lowerIsBetter}
            onChange={(e) => onChange('lowerIsBetter', e.target.checked)}
            data-testid="metric-lower-is-better"
            className="rounded border-gray-300"
          />
          Lower is better (renders the metric with a ↓ direction).
        </label>
      </div>
    </fieldset>
  );
}
