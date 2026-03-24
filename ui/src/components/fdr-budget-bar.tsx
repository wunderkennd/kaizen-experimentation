'use client';

import { memo, useEffect, useState } from 'react';
import type { OnlineFdrState } from '@/lib/types';
import { getOnlineFdrState, RpcError } from '@/lib/api';

interface FdrBudgetBarProps {
  experimentId: string;
}

/** Fraction of initial wealth below which a warning is shown. */
const WARN_THRESHOLD = 0.20;

/**
 * Progress bar showing the alpha wealth remaining in the online FDR controller
 * (e-LOND, ADR-018). Warns in orange when wealth drops below 20% of initial.
 */
function FdrBudgetBarInner({ experimentId }: FdrBudgetBarProps) {
  const [state, setState] = useState<OnlineFdrState | null>(null);

  useEffect(() => {
    getOnlineFdrState(experimentId)
      .then(setState)
      .catch((err) => {
        if (!(err instanceof RpcError && err.status === 404)) {
          console.error('FdrBudgetBar:', err);
        }
      });
  }, [experimentId]);

  if (!state) return null;

  const fraction = state.initialWealth > 0
    ? Math.max(0, Math.min(1, state.alphaWealth / state.initialWealth))
    : 0;
  const pct = fraction * 100;
  const isLow = fraction < WARN_THRESHOLD;

  const barColor = isLow ? 'bg-orange-400' : 'bg-indigo-500';
  const textColor = isLow ? 'text-orange-700' : 'text-indigo-700';
  const bgColor = isLow ? 'bg-orange-50 border-orange-200' : 'bg-white border-gray-200';

  return (
    <div
      className={`rounded-lg border px-4 py-3 ${bgColor}`}
      data-testid="fdr-budget-bar"
      aria-label={`Online FDR alpha wealth: ${pct.toFixed(1)}% remaining`}
    >
      <div className="mb-1 flex items-center justify-between">
        <span className="text-xs font-medium uppercase text-gray-500">
          Online FDR Alpha Wealth (ADR-018)
        </span>
        {isLow && (
          <span className="rounded-full bg-orange-100 px-2 py-0.5 text-xs font-medium text-orange-700">
            Low budget
          </span>
        )}
      </div>

      {/* Progress bar */}
      <div className="mb-2 h-2 w-full overflow-hidden rounded-full bg-gray-100" role="progressbar"
        aria-valuenow={Math.round(pct)} aria-valuemin={0} aria-valuemax={100}
      >
        <div
          className={`h-full rounded-full transition-all ${barColor}`}
          style={{ width: `${pct}%` }}
        />
      </div>

      {/* Numeric summary */}
      <div className="flex flex-wrap items-center justify-between gap-x-4 text-xs text-gray-500">
        <span>
          Wealth remaining:{' '}
          <span className={`font-medium ${textColor}`}>
            {state.alphaWealth.toFixed(4)}
          </span>
          {' '}/ {state.initialWealth.toFixed(4)} ({pct.toFixed(1)}%)
        </span>
        <span>
          Tested: {state.numTested} · Rejected: {state.numRejected}
          {' '}· Est. FDR: {(state.currentFdr * 100).toFixed(1)}%
        </span>
      </div>
    </div>
  );
}

export const FdrBudgetBar = memo(FdrBudgetBarInner);
