import { memo } from 'react';
import type { SlateAssignmentResponse } from '@/lib/types';

interface SlateResultsPanelProps {
  response: SlateAssignmentResponse;
}

const PROB_COLORS = [
  'bg-green-100 text-green-800',
  'bg-blue-100 text-blue-800',
  'bg-indigo-100 text-indigo-800',
  'bg-purple-100 text-purple-800',
  'bg-gray-100 text-gray-700',
];

function probColor(prob: number): string {
  if (prob >= 0.8) return PROB_COLORS[0];
  if (prob >= 0.6) return PROB_COLORS[1];
  if (prob >= 0.4) return PROB_COLORS[2];
  if (prob >= 0.2) return PROB_COLORS[3];
  return PROB_COLORS[4];
}

export const SlateResultsPanel = memo(function SlateResultsPanel({ response }: SlateResultsPanelProps) {
  const { slateItemIds, slotProbabilities, slateProbability } = response;

  return (
    <div data-testid="slate-results-panel">
      <h3 className="mb-3 text-sm font-semibold text-gray-900">Slate Assignment</h3>
      <ol className="space-y-2">
        {slateItemIds.map((itemId, i) => {
          const prob = slotProbabilities[i] ?? 0;
          return (
            <li
              key={`${itemId}-${i}`}
              className="flex items-center gap-3 rounded-md border border-gray-200 bg-white px-3 py-2"
            >
              <span className="flex h-6 w-6 flex-shrink-0 items-center justify-center rounded-full bg-indigo-600 text-xs font-bold text-white">
                {i + 1}
              </span>
              <span className="flex-1 truncate font-mono text-sm text-gray-800">{itemId}</span>
              <span
                className={`inline-flex items-center rounded-full px-2 py-0.5 text-xs font-medium ${probColor(prob)}`}
                title={`Slot probability: ${(prob * 100).toFixed(1)}%`}
              >
                {(prob * 100).toFixed(1)}%
              </span>
            </li>
          );
        })}
      </ol>
      <p className="mt-3 text-xs text-gray-500">
        Overall slate probability:{' '}
        <span className="font-medium text-gray-700">{slateProbability.toExponential(3)}</span>
      </p>
    </div>
  );
});
