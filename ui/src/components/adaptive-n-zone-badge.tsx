'use client';

import { memo } from 'react';
import type { AdaptiveNResult, AdaptiveNZone } from '@/lib/types';

interface ZoneConfig {
  label: string;
  bg: string;
  text: string;
  dot: string;
  title: string;
}

const ZONE_CONFIG: Record<AdaptiveNZone, ZoneConfig> = {
  FAVORABLE: {
    label: 'Favorable',
    bg: 'bg-green-100',
    text: 'text-green-800',
    dot: 'bg-green-500',
    title: 'Effect exceeds MDE. Consider early stopping.',
  },
  PROMISING: {
    label: 'Promising',
    bg: 'bg-yellow-100',
    text: 'text-yellow-800',
    dot: 'bg-yellow-500',
    title: 'Trend positive but underpowered. Timeline extension recommended.',
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

interface AdaptiveNZoneBadgeProps {
  result: AdaptiveNResult;
}

function AdaptiveNZoneBadgeInner({ result }: AdaptiveNZoneBadgeProps) {
  const cfg = ZONE_CONFIG[result.zone];

  const recText = result.recommendedN != null ? `. Recommended N: ${result.recommendedN.toLocaleString()}` : '';

  return (
    <span
      className={`inline-flex cursor-help items-center gap-1.5 rounded-full px-2.5 py-0.5 text-xs font-medium ${cfg.bg} ${cfg.text} focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500 focus-visible:ring-offset-1`}
      tabIndex={0}
      role="note"
      title={cfg.title}
      aria-label={`Adaptive N zone ${cfg.label}: ${cfg.title}${recText}`}
      data-testid="adaptive-n-zone-badge"
      data-zone={result.zone}
    >
      <span className={`h-1.5 w-1.5 rounded-full ${cfg.dot}`} aria-hidden="true" />
      Adaptive N: {cfg.label}
      {result.recommendedN != null && (
        <span className="font-semibold">
          {' '}· Rec. N: {result.recommendedN.toLocaleString()}
        </span>
      )}
    </span>
  );
}

export const AdaptiveNZoneBadge = memo(AdaptiveNZoneBadgeInner);
