'use client';

import { useEffect, useState } from 'react';
import type { AdaptiveNZone, AdaptiveNResult } from '@/lib/types';
import { getAdaptiveN, RpcError } from '@/lib/api';

const ZONE_CONFIG: Record<AdaptiveNZone, { label: string; bg: string; text: string; dot: string; title: string }> = {
  FAVORABLE: {
    label: 'Favorable',
    bg: 'bg-green-100',
    text: 'text-green-800',
    dot: 'bg-green-500',
    title: 'Effect exceeds MDE. Consider early stopping.',
  },
  PROMISING: {
    label: 'Promising',
    bg: 'bg-blue-100',
    text: 'text-blue-800',
    dot: 'bg-blue-500',
    title: 'Trend is positive but underpowered. Timeline extension recommended.',
  },
  FUTILE: {
    label: 'Futile',
    bg: 'bg-red-100',
    text: 'text-red-800',
    dot: 'bg-red-500',
    title: 'Conditional power too low to detect MDE. Consider stopping.',
  },
  INCONCLUSIVE: {
    label: 'Inconclusive',
    bg: 'bg-gray-100',
    text: 'text-gray-700',
    dot: 'bg-gray-400',
    title: 'Insufficient data to classify zone.',
  },
};

interface AdaptiveNBadgeProps {
  experimentId: string;
}

export function AdaptiveNBadge({ experimentId }: AdaptiveNBadgeProps) {
  const [result, setResult] = useState<AdaptiveNResult | null>(null);

  useEffect(() => {
    getAdaptiveN(experimentId)
      .then(setResult)
      .catch((err) => {
        // 404 = no adaptive-N data (experiment not using adaptive design)
        if (!(err instanceof RpcError && err.status === 404)) {
          console.error('AdaptiveNBadge:', err);
        }
      });
  }, [experimentId]);

  if (!result) return null;

  const cfg = ZONE_CONFIG[result.zone];

  return (
    <span
      className={`inline-flex items-center gap-1.5 rounded-full px-2.5 py-0.5 text-xs font-medium ${cfg.bg} ${cfg.text}`}
      title={cfg.title}
      data-testid="adaptive-n-badge"
      data-zone={result.zone}
    >
      <span className={`h-1.5 w-1.5 rounded-full ${cfg.dot}`} aria-hidden="true" />
      Adaptive N: {cfg.label}
    </span>
  );
}
