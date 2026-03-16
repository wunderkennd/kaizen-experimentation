'use client';

import { memo } from 'react';

interface IpwToggleProps {
  enabled: boolean;
  onToggle: () => void;
}

function IpwToggleInner({ enabled, onToggle }: IpwToggleProps) {
  return (
    <div className="mb-4 flex items-center gap-3">
      <label className="flex cursor-pointer items-center gap-2">
        <span className="text-sm font-medium text-gray-700">IPW Adjustment</span>
        <button
          role="switch"
          aria-checked={enabled}
          onClick={onToggle}
          className={`relative inline-flex h-6 w-11 shrink-0 cursor-pointer rounded-full border-2 border-transparent transition-colors ${
            enabled ? 'bg-amber-600' : 'bg-gray-200'
          }`}
        >
          <span
            className={`pointer-events-none inline-block h-5 w-5 rounded-full bg-white shadow ring-0 transition-transform ${
              enabled ? 'translate-x-5' : 'translate-x-0'
            }`}
          />
        </button>
      </label>
      {enabled && (
        <span className="inline-flex items-center rounded-full bg-amber-50 px-2.5 py-0.5 text-xs font-medium text-amber-700">
          Corrects for non-uniform assignment
        </span>
      )}
    </div>
  );
}

export const IpwToggle = memo(IpwToggleInner);
