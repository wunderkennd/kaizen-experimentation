'use client';

import type { ExperimentType } from '@/lib/types';
import { TYPE_LABELS, TYPE_DESCRIPTIONS } from '@/lib/utils';

interface TypeBadgeProps {
  type: ExperimentType;
}

/**
 * TypeBadge renders a styled badge with a descriptive tooltip explaining
 * the experiment design strategy on hover and keyboard focus.
 */
export function TypeBadge({ type }: TypeBadgeProps) {
  const label = TYPE_LABELS[type] || type;
  const description = TYPE_DESCRIPTIONS[type] || label;
  const ariaLabel = `${label}: ${description}`;

  return (
    <span
      tabIndex={0}
      className="inline-flex cursor-help items-center rounded-md bg-indigo-50 px-2 py-1 text-xs font-medium text-indigo-700 ring-1 ring-inset ring-indigo-600/20 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500 focus-visible:ring-offset-1"
      title={description}
      aria-label={ariaLabel}
      data-testid="type-badge"
      data-type={type}
    >
      {label}
    </span>
  );
}
