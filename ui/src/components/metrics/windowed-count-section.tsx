'use client';

import type { WindowedCountConfig } from '@/lib/types';
import { validateWindowedCountConfig } from '@/lib/validation';
import { useDebouncedValidation } from '@/hooks/use-debounced-validation';
import { SqlEditor } from './sql-editor';

interface WindowedCountSectionProps {
  value: WindowedCountConfig | undefined;
  onChange: (next: WindowedCountConfig) => void;
  disabled?: boolean;
}

const DEFAULT_CONFIG: WindowedCountConfig = {
  eventType: '',
  filterSql: '',
  windowHours: 24,
};

export function WindowedCountSection({ value, onChange, disabled }: WindowedCountSectionProps) {
  const cfg = value ?? DEFAULT_CONFIG;
  const validation = useDebouncedValidation(cfg, validateWindowedCountConfig);

  return (
    <fieldset disabled={disabled} className="flex flex-col gap-4 rounded border border-rose-200 bg-rose-50/30 p-4">
      <legend className="px-2 text-sm font-semibold text-rose-900">WINDOWED_COUNT</legend>

      <div className="flex flex-col gap-1">
        <label htmlFor="wc-event-type" className="text-sm font-medium text-gray-700">Event type</label>
        <input
          id="wc-event-type"
          type="text"
          value={cfg.eventType}
          onChange={(e) => onChange({ ...cfg, eventType: e.target.value })}
          placeholder="signup_completed"
          className="rounded border border-gray-300 px-3 py-2 font-mono text-sm focus:border-indigo-500 focus:outline-none"
        />
        <p className="text-xs text-gray-500">
          lowercase identifier — the event whose occurrences are counted per user
        </p>
      </div>

      <div className="flex flex-col gap-1">
        <label htmlFor="wc-window-hours" className="text-sm font-medium text-gray-700">Window (hours)</label>
        <input
          id="wc-window-hours"
          type="number"
          min={1}
          max={8760}
          step={1}
          value={cfg.windowHours}
          onChange={(e) => onChange({ ...cfg, windowHours: Number(e.target.value) })}
          className="rounded border border-gray-300 px-3 py-2 text-sm focus:border-indigo-500 focus:outline-none"
        />
        <p className="text-xs text-gray-500">
          Window is <strong>per-user exposure-anchored</strong> — starts at each user&apos;s first exposure to the experiment. 1 ≤ hours ≤ 8760 (1 year).
        </p>
      </div>

      <div className="flex flex-col gap-1">
        <label className="text-sm font-medium text-gray-700">Filter SQL <span className="text-gray-400">(optional)</span></label>
        <SqlEditor
          value={cfg.filterSql}
          onChange={(next) => onChange({ ...cfg, filterSql: next })}
          placeholder="platform = 'mobile'"
          ariaLabel="WINDOWED_COUNT optional filter SQL predicate"
          disabled={disabled}
        />
        <p className="text-xs text-gray-500">
          Optional WHERE-clause predicate; same operator allowlist as FILTERED_MEAN. Leave empty to count all matching events in the window.
        </p>
      </div>

      {validation.status === 'invalid' && (
        <p className="text-sm text-red-700" role="alert">{validation.error}</p>
      )}
    </fieldset>
  );
}
