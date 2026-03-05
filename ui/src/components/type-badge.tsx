'use client';

import type { ExperimentType } from '@/lib/types';
import { TYPE_LABELS } from '@/lib/utils';

interface TypeBadgeProps {
  type: ExperimentType;
}

export function TypeBadge({ type }: TypeBadgeProps) {
  return (
    <span className="inline-flex items-center rounded-md bg-indigo-50 px-2 py-1 text-xs font-medium text-indigo-700 ring-1 ring-inset ring-indigo-600/20">
      {TYPE_LABELS[type]}
    </span>
  );
}
