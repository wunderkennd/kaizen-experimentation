'use client';

import { memo, useEffect, useState } from 'react';
import type { FeedbackLoopResult } from '@/lib/types';
import { getFeedbackLoopAnalysis, RpcError } from '@/lib/api';
import { formatDate } from '@/lib/utils';

interface FeedbackLoopAlertProps {
  experimentId: string;
  primaryMetricId?: string;
}

function FeedbackLoopAlertInner({ experimentId, primaryMetricId }: FeedbackLoopAlertProps) {
  const [result, setResult] = useState<FeedbackLoopResult | null>(null);

  useEffect(() => {
    getFeedbackLoopAnalysis(experimentId)
      .then(setResult)
      .catch((err) => {
        if (!(err instanceof RpcError && err.status === 404)) {
          // Best-effort: banner is non-blocking; suppress but don't crash
        }
      });
  }, [experimentId]);

  if (!result) return null;

  // Use explicit backend flag when available; infer from data otherwise
  const feedbackLoopDetected =
    result.feedbackLoopDetected ?? (result.retrainingEvents.length > 0 && result.contaminationFraction > 0);

  if (!feedbackLoopDetected) return null;

  const estimatedBias = Math.abs(result.rawEstimate - result.biasCorrectedEstimate);
  const isError = estimatedBias > 0.1;
  const severity = isError ? 'ERROR' : 'WARNING';

  const sortedEvents = [...result.retrainingEvents].sort(
    (a, b) => new Date(b.retrainedAt).getTime() - new Date(a.retrainedAt).getTime(),
  );
  const mostRecent = sortedEvents[0];

  let timeSinceRetrain = '';
  if (mostRecent) {
    const diffDays = Math.floor(
      (Date.now() - new Date(mostRecent.retrainedAt).getTime()) / (1000 * 60 * 60 * 24),
    );
    timeSinceRetrain = diffDays === 0 ? 'today' : `${diffDays} day${diffDays !== 1 ? 's' : ''} ago`;
  }

  const metricLabel = primaryMetricId ?? 'primary metric';
  const contaminationPct = (result.contaminationFraction * 100).toFixed(1);

  const colorSet = isError
    ? { banner: 'bg-red-50 border-red-300', heading: 'text-red-800', text: 'text-red-700' }
    : { banner: 'bg-yellow-50 border-yellow-300', heading: 'text-yellow-800', text: 'text-yellow-700' };

  return (
    <div
      className={`mb-4 rounded-lg border p-4 ${colorSet.banner}`}
      role="alert"
      aria-live="polite"
      data-testid="feedback-loop-alert"
      data-severity={severity}
    >
      <div className="flex items-start gap-3">
        <span className="text-lg leading-none" aria-hidden="true">&#9888;</span>
        <div className="flex-1">
          <h3 className={`font-semibold ${colorSet.heading}`}>
            {severity}: Feedback Loop Interference Detected
          </h3>
          <p className={`mt-1 text-sm ${colorSet.text}`}>
            Model retraining has contaminated the primary metric estimate.
            Use the bias-corrected estimate for decisions.
          </p>
          <p className={`mt-2 text-sm ${colorSet.text}`}>
            <span data-testid="alert-metric-name">Metric: {metricLabel}</span>
            {' · '}
            <span data-testid="alert-contamination">Contamination: {contaminationPct}%</span>
            {' · '}
            <span data-testid="alert-bias">Bias: {estimatedBias.toFixed(4)}</span>
            {mostRecent && (
              <>
                {' · '}
                <span data-testid="alert-retrain-time">
                  Last retrain: {formatDate(mostRecent.retrainedAt)} ({timeSinceRetrain})
                </span>
              </>
            )}
          </p>
        </div>
      </div>
    </div>
  );
}

export const FeedbackLoopAlert = memo(FeedbackLoopAlertInner);
