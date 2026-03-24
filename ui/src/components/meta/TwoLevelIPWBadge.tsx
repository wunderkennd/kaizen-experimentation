'use client';

import { memo } from 'react';

interface TwoLevelIPWBadgeProps {
  variantProbability: number;
  armProbability: number;
  className?: string;
}

function TwoLevelIPWBadgeInner({ variantProbability, armProbability, className = '' }: TwoLevelIPWBadgeProps) {
  const compound = variantProbability * armProbability;

  return (
    <span
      data-testid="two-level-ipw-badge"
      title={`Two-level IPW: P(variant)=${variantProbability.toFixed(4)} × P(arm|variant)=${armProbability.toFixed(4)} = ${compound.toFixed(4)}`}
      className={`inline-flex items-center gap-1 rounded-full bg-amber-50 px-2 py-0.5 text-xs font-medium text-amber-800 ring-1 ring-amber-200 ${className}`}
    >
      <span>IPW</span>
      <span className="font-mono">{compound.toFixed(4)}</span>
      <span className="text-amber-500" aria-hidden="true">
        &#215;
      </span>
      <span className="sr-only">
        P(variant) {variantProbability.toFixed(4)} times P(arm|variant) {armProbability.toFixed(4)}
      </span>
    </span>
  );
}

export const TwoLevelIPWBadge = memo(TwoLevelIPWBadgeInner);
