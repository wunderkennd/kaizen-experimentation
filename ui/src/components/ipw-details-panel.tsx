'use client';

import { memo } from 'react';
import type { MetricResult } from '@/lib/types';
import { formatEffect, formatPValue } from '@/lib/utils';

interface IpwDetailsPanelProps {
  metricResults: MetricResult[];
}

function IpwDetailsPanelInner({ metricResults }: IpwDetailsPanelProps) {
  const ipwMetrics = metricResults.filter((m) => m.ipwResult);

  if (ipwMetrics.length === 0) return null;

  return (
    <section className="mb-6">
      <h2 className="mb-3 text-lg font-semibold text-gray-900">IPW-Adjusted Analysis</h2>
      <p className="mb-3 text-xs text-gray-500">
        Inverse Probability Weighting corrects treatment effects for non-uniform assignment probabilities in bandit experiments.
      </p>
      <div className="overflow-hidden rounded-lg border border-gray-200 bg-white">
        <table className="min-w-full divide-y divide-gray-200">
          <thead className="bg-gray-50">
            <tr>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">Metric</th>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">IPW Effect</th>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">SE</th>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">95% CI</th>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">p-value</th>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">Eff. Sample Size</th>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">Clipped</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-gray-200 bg-white">
            {ipwMetrics.map((m) => {
              const ipw = m.ipwResult!;
              const isSignificant = ipw.pValue < 0.05;
              return (
                <tr
                  key={m.metricId}
                  className={isSignificant ? 'border-l-4 border-l-amber-500' : ''}
                >
                  <td className="whitespace-nowrap px-4 py-3 text-sm font-medium text-gray-900">
                    {m.metricId}
                  </td>
                  <td className="whitespace-nowrap px-4 py-3 text-sm text-gray-600">
                    {formatEffect(ipw.effect)}
                  </td>
                  <td className="whitespace-nowrap px-4 py-3 text-sm text-gray-600">
                    {ipw.se.toFixed(4)}
                  </td>
                  <td className="whitespace-nowrap px-4 py-3 text-sm text-gray-600">
                    [{formatEffect(ipw.ciLower)}, {formatEffect(ipw.ciUpper)}]
                  </td>
                  <td className="whitespace-nowrap px-4 py-3 text-sm text-gray-600">
                    {formatPValue(ipw.pValue)}
                  </td>
                  <td className="whitespace-nowrap px-4 py-3 text-sm text-gray-600">
                    {Math.round(ipw.effectiveSampleSize).toLocaleString()}
                  </td>
                  <td className="whitespace-nowrap px-4 py-3 text-sm text-gray-600">
                    {ipw.nClipped > 0 ? (
                      <span className="inline-flex items-center rounded-full bg-yellow-100 px-2 py-0.5 text-xs font-medium text-yellow-800">
                        {ipw.nClipped}
                      </span>
                    ) : (
                      <span className="text-gray-400">0</span>
                    )}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    </section>
  );
}

export const IpwDetailsPanel = memo(IpwDetailsPanelInner);
