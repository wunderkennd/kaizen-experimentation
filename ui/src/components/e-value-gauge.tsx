'use client';

import { memo } from 'react';
import type { EValueResult } from '@/lib/types';

interface EValueGaugeProps {
  eValueResult: EValueResult;
}

/** e-value above which the gauge turns yellow (strong evidence zone). */
const STRONG_THRESHOLD = 5;

/** Log10 upper bound for the gauge arc full deflection (e = 1000). */
const LOG10_MAX = 3;

function gaugeColor(rejected: boolean, eValue: number): string {
  if (rejected) return '#ef4444';
  if (eValue > STRONG_THRESHOLD) return '#eab308';
  return '#9ca3af';
}

function evidenceLabel(rejected: boolean, eValue: number): string {
  if (rejected) return 'Null Rejected';
  if (eValue > STRONG_THRESHOLD) return 'Strong Evidence';
  return 'Insufficient Evidence';
}

/**
 * Semi-circle SVG gauge displaying the current e-value on a log scale,
 * implied significance level, and rejection status (ADR-018).
 *
 * Color coding:
 *   Red    → null rejected (eValue ≥ 1/alpha)
 *   Yellow → strong evidence (eValue > 5)
 *   Grey   → insufficient evidence
 */
function EValueGaugeInner({ eValueResult }: EValueGaugeProps) {
  const { eValue, impliedLevel, reject, alpha } = eValueResult;

  // Map log10(eValue) to 0–1 for the gauge arc (clamp to [0, LOG10_MAX]).
  const log10Val = Math.max(0, Math.min(LOG10_MAX, Math.log10(Math.max(eValue, 1))));
  const arcFraction = log10Val / LOG10_MAX;

  const color = gaugeColor(reject, eValue);
  const label = evidenceLabel(reject, eValue);

  // SVG semi-circle gauge parameters
  const r = 36;
  const cx = 60;
  const cy = 56;
  const circumference = Math.PI * r;              // half-circle arc length
  const trackDash = `${circumference} ${circumference}`;
  const valueDash = `${arcFraction * circumference} ${circumference}`;

  return (
    <div
      className="flex flex-col items-center rounded-lg border border-gray-200 bg-white px-4 py-3"
      data-testid="e-value-gauge"
      aria-label={`E-value gauge: ${eValue.toFixed(2)}, ${label}`}
    >
      <span className="mb-1 text-xs font-medium uppercase text-gray-500">
        E-Value (ADR-018)
      </span>

      {/* Semi-circle arc gauge */}
      <svg width={120} height={68} aria-hidden="true" role="img">
        {/* Background track */}
        <path
          d={`M ${cx - r} ${cy} A ${r} ${r} 0 0 1 ${cx + r} ${cy}`}
          fill="none"
          stroke="#f3f4f6"
          strokeWidth={10}
          strokeLinecap="round"
        />
        {/* Value arc */}
        <path
          d={`M ${cx - r} ${cy} A ${r} ${r} 0 0 1 ${cx + r} ${cy}`}
          fill="none"
          stroke={color}
          strokeWidth={10}
          strokeLinecap="round"
          strokeDasharray={trackDash}
          strokeDashoffset={String((1 - arcFraction) * circumference)}
        />
        {/* Center text */}
        <text
          x={cx}
          y={cy - 8}
          textAnchor="middle"
          fontSize={14}
          fontWeight="bold"
          fill={color}
        >
          {eValue >= 1000
            ? `${(eValue / 1000).toFixed(1)}k`
            : eValue.toFixed(1)}
        </text>
      </svg>

      {/* Status badge */}
      <span
        className={`-mt-1 rounded-full px-2 py-0.5 text-xs font-medium ${
          reject
            ? 'bg-red-100 text-red-700'
            : eValue > STRONG_THRESHOLD
            ? 'bg-yellow-100 text-yellow-700'
            : 'bg-gray-100 text-gray-600'
        }`}
      >
        {label}
      </span>

      {/* Numeric details */}
      <dl className="mt-2 grid grid-cols-2 gap-x-4 text-center text-xs text-gray-500">
        <div>
          <dt className="font-medium text-gray-400">Implied p</dt>
          <dd>{impliedLevel < 0.001 ? '<0.001' : impliedLevel.toFixed(3)}</dd>
        </div>
        <div>
          <dt className="font-medium text-gray-400">Reject at α={alpha}</dt>
          <dd>{reject ? 'Yes' : `No (need ≥${(1 / alpha).toFixed(0)})`}</dd>
        </div>
      </dl>
    </div>
  );
}

export const EValueGauge = memo(EValueGaugeInner);
