'use client';

import { memo } from 'react';
import type { EquivalenceResult, EquivalenceStatus } from '@/lib/types';

interface EquivalenceResultBadgeProps {
  result?: EquivalenceResult;
}

/**
 * ADR-027: Derive the equivalence verdict from a TOST result.
 *
 * Per ADR-027 §6: green when the (1−2α) CI falls entirely within the
 * equivalence margin, red when the CI lies entirely outside it, yellow when
 * the CI straddles a margin boundary. The Rust `equivalent` flag is the source
 * of truth for the green case (CI ⊂ (−δ, +δ)); geometry decides red vs yellow.
 * This is presentation logic only — no statistics are computed here.
 */
export function deriveEquivalenceStatus(result: EquivalenceResult): EquivalenceStatus {
  if (result.equivalent) return 'EQUIVALENT';
  // CI lies entirely beyond the equivalence zone → definitively not equivalent.
  if (result.ciLower > result.delta || result.ciUpper < -result.delta) {
    return 'NOT_EQUIVALENT';
  }
  // CI overlaps a margin boundary → not enough evidence either way.
  return 'INCONCLUSIVE';
}

const STATUS_CONFIG: Record<
  EquivalenceStatus,
  { label: string; bg: string; text: string; dot: string; title: string }
> = {
  EQUIVALENT: {
    label: 'Equivalent',
    bg: 'bg-green-100',
    text: 'text-green-800',
    dot: 'bg-green-500',
    title:
      'The (1−2α) confidence interval falls entirely within ±δ. Affirmative evidence of no meaningful impact — safe to migrate.',
  },
  INCONCLUSIVE: {
    label: 'Inconclusive',
    bg: 'bg-yellow-100',
    text: 'text-yellow-800',
    dot: 'bg-yellow-500',
    title:
      'The confidence interval straddles a margin boundary. Equivalence is neither established nor ruled out — collect more data.',
  },
  NOT_EQUIVALENT: {
    label: 'Not Equivalent',
    bg: 'bg-red-100',
    text: 'text-red-800',
    dot: 'bg-red-500',
    title:
      'The confidence interval extends beyond ±δ. The effect exceeds the acceptable margin — not safe to migrate.',
  },
};

function EquivalenceResultBadgeInner({ result }: EquivalenceResultBadgeProps) {
  if (!result) {
    return (
      <span
        className="inline-flex items-center gap-1.5 rounded-full bg-gray-100 px-2.5 py-0.5 text-xs font-medium text-gray-700"
        title="Awaiting equivalence test computation."
        data-testid="equivalence-result-badge"
        data-status="PENDING"
      >
        <span className="h-1.5 w-1.5 rounded-full bg-gray-400" aria-hidden="true" />
        Equivalence Pending
      </span>
    );
  }

  const status = deriveEquivalenceStatus(result);
  const cfg = STATUS_CONFIG[status];

  return (
    <span
      className={`inline-flex items-center gap-1.5 rounded-full px-2.5 py-0.5 text-xs font-medium ${cfg.bg} ${cfg.text}`}
      title={cfg.title}
      data-testid="equivalence-result-badge"
      data-status={status}
    >
      <span className={`h-1.5 w-1.5 rounded-full ${cfg.dot}`} aria-hidden="true" />
      {cfg.label}
    </span>
  );
}

export const EquivalenceResultBadge = memo(EquivalenceResultBadgeInner);
