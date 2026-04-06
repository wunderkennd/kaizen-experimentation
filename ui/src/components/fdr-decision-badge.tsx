'use client';

import { memo } from 'react';
import type { EValueResult, OnlineFdrState, FdrDecision } from '@/lib/types';

interface FdrDecisionBadgeProps {
  eValueResult?: EValueResult;
  fdrState?: OnlineFdrState | null;
}

const DECISION_CONFIG: Record<FdrDecision, { label: string; bg: string; text: string; dot: string; title: string }> = {
  PASS: {
    label: 'FDR Pass',
    bg: 'bg-green-100',
    text: 'text-green-800',
    dot: 'bg-green-500',
    title: 'Null rejected and FDR is under control. Safe to act on this result.',
  },
  FAIL: {
    label: 'FDR Fail',
    bg: 'bg-red-100',
    text: 'text-red-800',
    dot: 'bg-red-500',
    title: 'Insufficient evidence to reject the null after FDR correction.',
  },
  PENDING: {
    label: 'FDR Pending',
    bg: 'bg-gray-100',
    text: 'text-gray-700',
    dot: 'bg-gray-400',
    title: 'Awaiting e-value computation or FDR evaluation.',
  },
};

function deriveDecision(eValueResult?: EValueResult, fdrState?: OnlineFdrState | null): FdrDecision {
  if (!eValueResult) return 'PENDING';

  // If online FDR is available, use it: reject only if both the e-value rejects
  // and the current FDR is below the target alpha (cross-experiment control).
  if (fdrState) {
    if (eValueResult.reject && fdrState.currentFdr <= eValueResult.alpha) return 'PASS';
    if (!eValueResult.reject) return 'FAIL';
    // e-value rejects but FDR is over budget — conservative: fail
    return 'FAIL';
  }

  // No FDR state — use within-experiment e-value decision only.
  if (eValueResult.reject) return 'PASS';
  return 'FAIL';
}

function FdrDecisionBadgeInner({ eValueResult, fdrState }: FdrDecisionBadgeProps) {
  const decision = deriveDecision(eValueResult, fdrState);
  const cfg = DECISION_CONFIG[decision];

  return (
    <span
      className={`inline-flex items-center gap-1.5 rounded-full px-2.5 py-0.5 text-xs font-medium ${cfg.bg} ${cfg.text}`}
      title={cfg.title}
      data-testid="fdr-decision-badge"
      data-decision={decision}
    >
      <span className={`h-1.5 w-1.5 rounded-full ${cfg.dot}`} aria-hidden="true" />
      {cfg.label}
    </span>
  );
}

export const FdrDecisionBadge = memo(FdrDecisionBadgeInner);
