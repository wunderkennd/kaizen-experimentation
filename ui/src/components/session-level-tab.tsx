'use client';

import type { MetricResult } from '@/lib/types';

interface SessionLevelTabProps {
  metricResults: MetricResult[];
}

function designEffectBadge(de: number): { label: string; bg: string; text: string } {
  if (de <= 1.5) return { label: 'Low', bg: 'bg-green-100', text: 'text-green-800' };
  if (de <= 3.0) return { label: 'Moderate', bg: 'bg-yellow-100', text: 'text-yellow-800' };
  return { label: 'High', bg: 'bg-red-100', text: 'text-red-800' };
}

const SIGNIFICANCE_THRESHOLD = 0.05;

export function SessionLevelTab({ metricResults }: SessionLevelTabProps) {
  if (metricResults.length === 0) {
    return (
      <div className="rounded-md bg-gray-50 p-4 text-sm text-gray-500">
        No session-level analysis available for this experiment.
      </div>
    );
  }

  return (
    <div className="space-y-4">
      <div className="rounded-md bg-blue-50 border border-blue-200 p-4">
        <h4 className="text-sm font-semibold text-blue-800">Session-Level Analysis</h4>
        <p className="mt-1 text-sm text-blue-700">
          Compares naive and HC1-clustered standard errors to account for within-session
          correlation. The design effect measures how much clustering inflates variance
          relative to the naive (IID) assumption.
        </p>
      </div>

      <div className="overflow-x-auto">
        <table className="min-w-full divide-y divide-gray-200">
          <thead>
            <tr className="bg-gray-50">
              <th className="px-4 py-3 text-left text-xs font-medium uppercase text-gray-500">Metric</th>
              <th className="px-4 py-3 text-right text-xs font-medium uppercase text-gray-500">Naive SE</th>
              <th className="px-4 py-3 text-right text-xs font-medium uppercase text-gray-500">Clustered SE</th>
              <th className="px-4 py-3 text-center text-xs font-medium uppercase text-gray-500">Design Effect</th>
              <th className="px-4 py-3 text-right text-xs font-medium uppercase text-gray-500">Naive p-value</th>
              <th className="px-4 py-3 text-right text-xs font-medium uppercase text-gray-500">Clustered p-value</th>
              <th className="px-4 py-3 text-center text-xs font-medium uppercase text-gray-500">Significance Shift</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-gray-200 bg-white">
            {metricResults.map((m) => {
              const sl = m.sessionLevelResult!;
              const badge = designEffectBadge(sl.designEffect);
              const naiveSig = sl.naivePValue < SIGNIFICANCE_THRESHOLD;
              const clusteredSig = sl.clusteredPValue < SIGNIFICANCE_THRESHOLD;
              const underEstimated = naiveSig && !clusteredSig;
              return (
                <tr key={m.metricId}>
                  <td className="whitespace-nowrap px-4 py-3 text-sm font-medium text-gray-900">
                    {m.metricId}
                  </td>
                  <td className="whitespace-nowrap px-4 py-3 text-right text-sm font-mono text-gray-700">
                    {sl.naiveSe.toFixed(3)}
                  </td>
                  <td className="whitespace-nowrap px-4 py-3 text-right text-sm font-mono text-gray-700">
                    {sl.clusteredSe.toFixed(3)}
                  </td>
                  <td className="whitespace-nowrap px-4 py-3 text-center">
                    <span className={`inline-flex rounded-full px-2 py-0.5 text-xs font-medium ${badge.bg} ${badge.text}`}>
                      {sl.designEffect.toFixed(1)} &mdash; {badge.label}
                    </span>
                  </td>
                  <td className={`whitespace-nowrap px-4 py-3 text-right text-sm font-mono ${
                    naiveSig ? 'text-green-700 font-semibold' : 'text-gray-700'
                  }`}>
                    {sl.naivePValue.toFixed(4)}
                  </td>
                  <td className={`whitespace-nowrap px-4 py-3 text-right text-sm font-mono ${
                    clusteredSig ? 'text-green-700 font-semibold' : 'text-gray-700'
                  }`}>
                    {sl.clusteredPValue.toFixed(4)}
                  </td>
                  <td className="whitespace-nowrap px-4 py-3 text-center">
                    {underEstimated ? (
                      <span className="inline-flex items-center rounded-full bg-orange-100 px-2 py-0.5 text-xs font-medium text-orange-800">
                        Under-estimated
                      </span>
                    ) : (
                      <span className="text-xs text-gray-400">&mdash;</span>
                    )}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>

      <div className="rounded-lg border border-gray-200 bg-white p-4">
        <h4 className="text-sm font-semibold text-gray-900">Design Effect Guide</h4>
        <div className="mt-2 flex items-center gap-6 text-xs text-gray-600">
          <span className="flex items-center gap-1.5">
            <span className="inline-block h-2.5 w-2.5 rounded-full bg-green-500" aria-hidden="true" />
            Low (&le; 1.5)
          </span>
          <span className="flex items-center gap-1.5">
            <span className="inline-block h-2.5 w-2.5 rounded-full bg-yellow-500" aria-hidden="true" />
            Moderate (1.5 &ndash; 3.0)
          </span>
          <span className="flex items-center gap-1.5">
            <span className="inline-block h-2.5 w-2.5 rounded-full bg-red-500" aria-hidden="true" />
            High (&gt; 3.0)
          </span>
        </div>
      </div>
    </div>
  );
}
