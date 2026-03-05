'use client';

interface CupedToggleProps {
  enabled: boolean;
  onToggle: () => void;
  varianceReductionPct: number;
}

export function CupedToggle({ enabled, onToggle, varianceReductionPct }: CupedToggleProps) {
  return (
    <div className="mb-4 flex items-center gap-3">
      <label className="flex cursor-pointer items-center gap-2">
        <span className="text-sm font-medium text-gray-700">CUPED Adjustment</span>
        <button
          role="switch"
          aria-checked={enabled}
          onClick={onToggle}
          className={`relative inline-flex h-6 w-11 shrink-0 cursor-pointer rounded-full border-2 border-transparent transition-colors ${
            enabled ? 'bg-indigo-600' : 'bg-gray-200'
          }`}
        >
          <span
            className={`pointer-events-none inline-block h-5 w-5 rounded-full bg-white shadow ring-0 transition-transform ${
              enabled ? 'translate-x-5' : 'translate-x-0'
            }`}
          />
        </button>
      </label>
      {varianceReductionPct > 0 && (
        <span className="inline-flex items-center rounded-full bg-indigo-50 px-2.5 py-0.5 text-xs font-medium text-indigo-700">
          ~{varianceReductionPct}% variance reduction
        </span>
      )}
    </div>
  );
}
