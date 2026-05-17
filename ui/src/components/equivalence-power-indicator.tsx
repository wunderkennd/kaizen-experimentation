'use client';

import { memo } from 'react';

interface EquivalencePowerIndicatorProps {
  /** Power at the current sample size, computed by experimentation-stats. */
  achievedPower?: number;
  /** Target power the design aims for (default 0.80). */
  targetPower?: number;
}

/**
 * ADR-027 §6: power indicator at the current sample size given δ. The value is
 * computed in experimentation-stats and passed through — this component renders
 * it only (no statistics in TypeScript per the platform rules). Equivalence
 * tests need ~2× the sample size of a superiority test, so underpowered TOST
 * runs are common and worth flagging.
 */
function EquivalencePowerIndicatorInner({
  achievedPower,
  targetPower = 0.8,
}: EquivalencePowerIndicatorProps) {
  if (achievedPower === undefined) {
    return (
      <div
        className="rounded-md border border-gray-200 bg-gray-50 px-3 py-2 text-xs text-gray-600"
        data-testid="equivalence-power-indicator"
        data-state="pending"
      >
        Power at current sample size is not yet available for this experiment.
      </div>
    );
  }

  const pct = Math.round(achievedPower * 100);
  const underpowered = achievedPower < targetPower;
  const barColor = underpowered ? 'bg-yellow-500' : 'bg-green-500';
  const width = `${Math.min(Math.max(achievedPower, 0), 1) * 100}%`;

  return (
    <div
      className="rounded-md border border-gray-200 bg-white px-3 py-2"
      data-testid="equivalence-power-indicator"
      data-state={underpowered ? 'underpowered' : 'adequate'}
    >
      <div className="mb-1 flex items-center justify-between text-xs">
        <span className="font-medium text-gray-700">
          Power at current sample size (given δ)
        </span>
        <span className="font-semibold text-gray-900" data-testid="equiv-power-value">
          {pct}%
        </span>
      </div>
      <div
        className="h-2 w-full overflow-hidden rounded-full bg-gray-200"
        role="progressbar"
        aria-valuenow={pct}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-label="Equivalence test power"
      >
        <div className={`h-full rounded-full ${barColor}`} style={{ width }} />
      </div>
      {underpowered && (
        <p className="mt-1.5 text-xs text-yellow-700">
          Below the {Math.round(targetPower * 100)}% target. Equivalence tests
          require ~2× the sample size of a standard test — consider extending the
          experiment duration before concluding.
        </p>
      )}
    </div>
  );
}

export const EquivalencePowerIndicator = memo(EquivalencePowerIndicatorInner);
