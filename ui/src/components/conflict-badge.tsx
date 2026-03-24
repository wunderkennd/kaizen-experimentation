'use client';

import type { PortfolioExperiment } from '@/lib/types';

interface ConflictBadgeProps {
  experiment: PortfolioExperiment;
  allExperiments: PortfolioExperiment[];
}

/** Returns the set of shared segments between this experiment and any other. */
function findConflictingSegments(
  experiment: PortfolioExperiment,
  allExperiments: PortfolioExperiment[],
): string[] {
  const shared = new Set<string>();
  for (const other of allExperiments) {
    if (other.experimentId === experiment.experimentId) continue;
    for (const seg of experiment.userSegments) {
      if (other.userSegments.includes(seg)) {
        shared.add(seg);
      }
    }
  }
  return Array.from(shared);
}

/** Highlights experiments that share user segments with other active experiments. */
export function ConflictBadge({ experiment, allExperiments }: ConflictBadgeProps) {
  const conflicts = findConflictingSegments(experiment, allExperiments);

  if (conflicts.length === 0) return null;

  return (
    <span
      className="inline-flex items-center rounded-full bg-amber-100 px-2 py-0.5 text-xs font-medium text-amber-800"
      title={`Shares segments with other experiments: ${conflicts.join(', ')}`}
      data-testid="conflict-badge"
      aria-label={`Segment conflict: ${conflicts.join(', ')}`}
    >
      ⚠ {conflicts.length} shared {conflicts.length === 1 ? 'segment' : 'segments'}
    </span>
  );
}
