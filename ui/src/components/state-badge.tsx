'use client';

import type { ExperimentState } from '@/lib/types';
import { STATE_CONFIG } from '@/lib/utils';

interface StateBadgeProps {
  state: ExperimentState;
}

export function StateBadge({ state }: StateBadgeProps) {
  const config = STATE_CONFIG[state];

  return (
    <span
      className={`inline-flex items-center gap-1.5 rounded-full px-2.5 py-0.5 text-xs font-medium ${config.bgColor} ${config.textColor} ${config.italic ? 'italic' : ''}`}
    >
      <span
        className={`h-2 w-2 rounded-full ${config.dotColor} ${config.animate ? 'animate-pulse-slow' : ''}`}
        aria-hidden="true"
      />
      {config.label}
    </span>
  );
}
