'use client';

import { useEffect, useState, useCallback } from 'react';
import { getSlateHeatmap } from '@/lib/api';
import type { SlateHeatmapResult } from '@/lib/types';
import { RetryableError } from '@/components/retryable-error';

interface SlateHeatmapProps {
  experimentId: string;
}

/**
 * Color-coded grid showing item×position assignment probabilities.
 * Uses CSS grid — Recharts doesn't natively support heatmaps.
 * Color scale: white (0.0) → indigo-100 → indigo-600 (1.0).
 */

function probabilityToColor(p: number): string {
  // Clamp to [0, 1]
  const v = Math.max(0, Math.min(1, p));
  if (v < 0.001) return '#ffffff';
  if (v < 0.1) return '#eef2ff';   // indigo-50
  if (v < 0.2) return '#e0e7ff';   // indigo-100
  if (v < 0.3) return '#c7d2fe';   // indigo-200
  if (v < 0.4) return '#a5b4fc';   // indigo-300
  if (v < 0.5) return '#818cf8';   // indigo-400
  if (v < 0.7) return '#6366f1';   // indigo-500
  return '#4f46e5';                  // indigo-600
}

function textColor(p: number): string {
  return p >= 0.4 ? '#ffffff' : '#374151';
}

export function SlateHeatmap({ experimentId }: SlateHeatmapProps) {
  const [result, setResult] = useState<SlateHeatmapResult | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const fetchData = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const data = await getSlateHeatmap(experimentId);
      setResult(data);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load slate heatmap data.');
    } finally {
      setLoading(false);
    }
  }, [experimentId]);

  useEffect(() => {
    fetchData();
  }, [fetchData]);

  if (loading && !result) {
    return (
      <div className="flex items-center justify-center py-8" role="status" aria-label="Loading slate heatmap">
        <div className="h-6 w-6 animate-spin rounded-full border-2 border-gray-300 border-t-indigo-600" />
        <span className="sr-only">Loading slate heatmap</span>
      </div>
    );
  }

  if (error && !result) {
    return <RetryableError message={error} onRetry={fetchData} context="slate heatmap" />;
  }

  if (!result || result.cells.length === 0) {
    return (
      <div className="rounded-lg border border-gray-200 bg-white p-6" data-testid="slate-heatmap">
        <h3 className="text-sm font-semibold text-gray-900">Slate Assignment Heatmap</h3>
        <p className="mt-4 text-center text-sm text-gray-500">No heatmap data available.</p>
      </div>
    );
  }

  // Build lookup: (itemId, position) → probability
  const cellMap = new Map<string, number>();
  for (const cell of result.cells) {
    cellMap.set(`${cell.itemId}:${cell.position}`, cell.probability);
  }

  return (
    <div className="rounded-lg border border-gray-200 bg-white p-4" data-testid="slate-heatmap">
      <h3 className="mb-3 text-sm font-semibold text-gray-900">Slate Assignment Heatmap</h3>
      <p className="mb-3 text-xs text-gray-500">
        Assignment probability per item per position — darker = higher probability
      </p>

      <div className="overflow-x-auto">
        <table className="border-collapse text-xs" role="grid" aria-label="Slate assignment probabilities">
          <thead>
            <tr>
              <th className="px-2 py-1.5 text-left text-gray-500 font-medium">Item \ Pos</th>
              {result.positions.map((pos) => (
                <th key={pos} className="px-2 py-1.5 text-center text-gray-500 font-medium min-w-[48px]">
                  {pos}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {result.items.map((itemId) => (
              <tr key={itemId}>
                <td className="px-2 py-1 font-mono text-gray-700 whitespace-nowrap" title={itemId}>
                  {itemId.length > 12 ? `${itemId.slice(0, 12)}...` : itemId}
                </td>
                {result.positions.map((pos) => {
                  const prob = cellMap.get(`${itemId}:${pos}`) ?? 0;
                  return (
                    <td
                      key={pos}
                      className="px-2 py-1 text-center font-mono border border-gray-100"
                      style={{
                        backgroundColor: probabilityToColor(prob),
                        color: textColor(prob),
                      }}
                      title={`${itemId} at position ${pos}: ${(prob * 100).toFixed(1)}%`}
                      data-testid={`heatmap-cell-${itemId}-${pos}`}
                    >
                      {prob > 0.001 ? (prob * 100).toFixed(0) : ''}
                    </td>
                  );
                })}
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      {/* Color scale legend */}
      <div className="mt-3 flex items-center gap-2 text-xs text-gray-500">
        <span>0%</span>
        <div className="flex h-3">
          {[0, 0.1, 0.2, 0.3, 0.4, 0.5, 0.7, 1.0].map((v) => (
            <span
              key={v}
              className="inline-block h-3 w-5 border border-gray-200"
              style={{ backgroundColor: probabilityToColor(v) }}
            />
          ))}
        </div>
        <span>100%</span>
      </div>

      {result.computedAt && (
        <p className="mt-2 text-xs text-gray-400" data-testid="heatmap-computed-at">
          Computed at: {new Date(result.computedAt).toLocaleString()}
        </p>
      )}
    </div>
  );
}
