'use client';

import { memo, useEffect, useState } from 'react';
import type { OptimalAlphaRecommendation } from '@/lib/types';
import { getOptimalAlpha, RpcError } from '@/lib/api';

interface OptimalAlphaWidgetProps {
  experimentId: string;
  currentAlpha: number;
}

/**
 * Displays the portfolio-level optimal alpha recommendation (ADR-019).
 *
 * Compares the experiment's current alpha with the portfolio-recommended alpha.
 * When the recommended alpha differs from the current setting, highlights the
 * recommendation with an explanation.
 */
function OptimalAlphaWidgetInner({ experimentId, currentAlpha }: OptimalAlphaWidgetProps) {
  const [rec, setRec] = useState<OptimalAlphaRecommendation | null>(null);

  useEffect(() => {
    getOptimalAlpha(experimentId)
      .then(setRec)
      .catch((err) => {
        if (!(err instanceof RpcError && err.status === 404)) {
          console.error('OptimalAlphaWidget:', err);
        }
      });
  }, [experimentId]);

  if (!rec) return null;

  const isRelaxed = rec.optimalAlpha > currentAlpha;
  const isStricter = rec.optimalAlpha < currentAlpha;
  const isSame = !isRelaxed && !isStricter;

  const borderColor = isRelaxed
    ? 'border-blue-200 bg-blue-50'
    : isStricter
    ? 'border-amber-200 bg-amber-50'
    : 'border-gray-200 bg-white';

  return (
    <div
      className={`rounded-lg border px-4 py-3 ${borderColor}`}
      data-testid="optimal-alpha-widget"
    >
      <div className="mb-1 flex items-center justify-between">
        <span className="text-xs font-medium uppercase text-gray-500">
          Optimal Alpha (ADR-019)
        </span>
        {!isSame && (
          <span className={`rounded-full px-2 py-0.5 text-xs font-medium ${
            isRelaxed ? 'bg-blue-100 text-blue-700' : 'bg-amber-100 text-amber-700'
          }`}>
            {isRelaxed ? 'Relaxation suggested' : 'Stricter threshold suggested'}
          </span>
        )}
      </div>

      <div className="flex items-baseline gap-4">
        <div>
          <span className="text-2xl font-bold text-gray-900">
            {rec.optimalAlpha.toFixed(3)}
          </span>
          <span className="ml-1 text-sm text-gray-500">recommended</span>
        </div>
        <div className="text-sm text-gray-500">
          vs. current <span className="font-medium text-gray-700">{currentAlpha.toFixed(3)}</span>
        </div>
      </div>

      <p className="mt-2 text-xs text-gray-500">
        {isRelaxed
          ? 'Portfolio analysis suggests a relaxed threshold may maximize total program value. Low-cost treatments benefit from running more experiments at higher alpha.'
          : isStricter
          ? 'Given the current FDR budget, a stricter threshold is recommended to maintain cross-experiment error control.'
          : 'Current alpha aligns with the portfolio-level recommendation.'}
      </p>

      <dl className="mt-2 flex gap-4 text-xs text-gray-500">
        <div>
          <dt className="font-medium text-gray-400">Expected FDR</dt>
          <dd>{(rec.expectedPortfolioFdr * 100).toFixed(1)}%</dd>
        </div>
      </dl>
    </div>
  );
}

export const OptimalAlphaWidget = memo(OptimalAlphaWidgetInner);
