'use client';

import { memo } from 'react';
import type { MetricResult } from '@/lib/types';
import { formatPValue, formatEffect } from '@/lib/utils';

interface TreatmentEffectsTableProps {
  metricResults: MetricResult[];
  showCuped: boolean;
}

function TreatmentEffectsTableInner({ metricResults, showCuped }: TreatmentEffectsTableProps) {
  return (
    <div className="overflow-hidden rounded-lg border border-gray-200 bg-white">
      <table className="min-w-full divide-y divide-gray-200">
        <thead className="bg-gray-50">
          <tr>
            <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">Metric</th>
            <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">Control</th>
            <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">Treatment</th>
            <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">Effect</th>
            <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">95% CI</th>
            <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">p-value</th>
            <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">Significance</th>
          </tr>
        </thead>
        <tbody className="divide-y divide-gray-200 bg-white">
          {metricResults.map((m) => {
            const effect = showCuped && m.varianceReductionPct > 0 ? m.cupedAdjustedEffect : m.absoluteEffect;
            const ciLow = showCuped && m.varianceReductionPct > 0 ? m.cupedCiLower : m.ciLower;
            const ciHigh = showCuped && m.varianceReductionPct > 0 ? m.cupedCiUpper : m.ciUpper;

            return (
              <tr
                key={m.metricId}
                className={m.isSignificant ? 'border-l-4 border-l-green-500' : ''}
              >
                <td className="whitespace-nowrap px-4 py-3 text-sm font-medium text-gray-900">
                  {m.metricId}
                </td>
                <td className="whitespace-nowrap px-4 py-3 text-sm text-gray-600">
                  {m.controlMean.toFixed(4)}
                </td>
                <td className="whitespace-nowrap px-4 py-3 text-sm text-gray-600">
                  {m.treatmentMean.toFixed(4)}
                </td>
                <td className="whitespace-nowrap px-4 py-3 text-sm text-gray-600">
                  {formatEffect(effect)}
                </td>
                <td className="whitespace-nowrap px-4 py-3 text-sm text-gray-600">
                  [{formatEffect(ciLow)}, {formatEffect(ciHigh)}]
                </td>
                <td className="whitespace-nowrap px-4 py-3 text-sm text-gray-600">
                  {formatPValue(m.pValue)}
                </td>
                <td className="whitespace-nowrap px-4 py-3 text-sm">
                  {m.isSignificant ? (
                    <span className="inline-flex items-center rounded-full bg-green-100 px-2.5 py-0.5 text-xs font-medium text-green-800">
                      Significant
                    </span>
                  ) : (
                    <span className="inline-flex items-center rounded-full bg-gray-100 px-2.5 py-0.5 text-xs font-medium text-gray-600">
                      Not Significant
                    </span>
                  )}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

export const TreatmentEffectsTable = memo(TreatmentEffectsTableInner);
