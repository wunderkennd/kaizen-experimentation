'use client';

import { useEffect, useState } from 'react';
import type { InterferenceAnalysisResult } from '@/lib/types';
import { getInterferenceAnalysis } from '@/lib/api';
import { formatPValue } from '@/lib/utils';

interface InterferenceTabProps {
  experimentId: string;
}

export function InterferenceTab({ experimentId }: InterferenceTabProps) {
  const [result, setResult] = useState<InterferenceAnalysisResult | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    getInterferenceAnalysis(experimentId)
      .then(setResult)
      .catch((err) => setError(err.message))
      .finally(() => setLoading(false));
  }, [experimentId]);

  if (loading) {
    return (
      <div className="flex items-center justify-center py-8">
        <div className="h-6 w-6 animate-spin rounded-full border-4 border-gray-300 border-t-indigo-600" />
      </div>
    );
  }

  if (error || !result) {
    return (
      <div className="rounded-md bg-gray-50 p-4 text-sm text-gray-500">
        No interference analysis available for this experiment.
      </div>
    );
  }

  return (
    <div className="space-y-4">
      {/* Detection banner */}
      {result.interferenceDetected ? (
        <div className="rounded-md bg-red-50 border border-red-200 p-4">
          <h4 className="text-sm font-semibold text-red-800">Content Interference Detected</h4>
          <p className="mt-1 text-sm text-red-700">
            Treatment and control groups show significant differences in content consumption patterns.
            This may indicate spillover effects between experiment arms.
          </p>
        </div>
      ) : (
        <div className="rounded-md bg-green-50 border border-green-200 p-4">
          <h4 className="text-sm font-semibold text-green-800">No Interference Detected</h4>
          <p className="mt-1 text-sm text-green-700">
            Content consumption patterns are similar across experiment arms.
          </p>
        </div>
      )}

      {/* Distribution metrics */}
      <div className="grid grid-cols-2 gap-4 sm:grid-cols-3">
        <div className="rounded-lg border border-gray-200 bg-white p-3">
          <span className="text-xs font-medium uppercase text-gray-500">JS Divergence</span>
          <p className="mt-1 text-lg font-semibold text-gray-900">
            {result.jensenShannonDivergence.toFixed(4)}
          </p>
        </div>
        <div className="rounded-lg border border-gray-200 bg-white p-3">
          <span className="text-xs font-medium uppercase text-gray-500">Jaccard Similarity (Top 100)</span>
          <p className="mt-1 text-lg font-semibold text-gray-900">
            {(result.jaccardSimilarityTop100 * 100).toFixed(1)}%
          </p>
        </div>
        <div className="rounded-lg border border-gray-200 bg-white p-3">
          <span className="text-xs font-medium uppercase text-gray-500">Catalog Coverage</span>
          <p className="mt-1 text-sm text-gray-900">
            Treatment: {(result.treatmentCatalogCoverage * 100).toFixed(1)}% /
            Control: {(result.controlCatalogCoverage * 100).toFixed(1)}%
          </p>
        </div>
      </div>

      {/* Gini comparison */}
      <div className="rounded-lg border border-gray-200 bg-white p-4">
        <h4 className="text-sm font-semibold text-gray-900">Concentration (Gini Coefficient)</h4>
        <div className="mt-3 space-y-2">
          <div className="flex items-center gap-3">
            <span className="w-24 text-xs text-gray-500">Treatment</span>
            <div className="flex-1 h-4 rounded bg-gray-100 overflow-hidden">
              <div
                className="h-full rounded bg-indigo-500"
                style={{ width: `${result.treatmentGiniCoefficient * 100}%` }}
              />
            </div>
            <span className="w-12 text-right text-xs font-medium text-gray-700">
              {result.treatmentGiniCoefficient.toFixed(2)}
            </span>
          </div>
          <div className="flex items-center gap-3">
            <span className="w-24 text-xs text-gray-500">Control</span>
            <div className="flex-1 h-4 rounded bg-gray-100 overflow-hidden">
              <div
                className="h-full rounded bg-gray-500"
                style={{ width: `${result.controlGiniCoefficient * 100}%` }}
              />
            </div>
            <span className="w-12 text-right text-xs font-medium text-gray-700">
              {result.controlGiniCoefficient.toFixed(2)}
            </span>
          </div>
        </div>
      </div>

      {/* Spillover titles table */}
      {result.spilloverTitles.length > 0 && (
        <div>
          <h4 className="mb-2 text-sm font-semibold text-gray-900">
            Spillover Titles ({result.spilloverTitles.length})
          </h4>
          <div className="overflow-hidden rounded-lg border border-gray-200 bg-white">
            <table className="min-w-full divide-y divide-gray-200">
              <thead className="bg-gray-50">
                <tr>
                  <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">Content ID</th>
                  <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">Treatment Rate</th>
                  <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">Control Rate</th>
                  <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">p-value</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-gray-200">
                {result.spilloverTitles.map((t) => (
                  <tr key={t.contentId}>
                    <td className="whitespace-nowrap px-4 py-3 text-sm font-medium text-gray-900">{t.contentId}</td>
                    <td className="whitespace-nowrap px-4 py-3 text-sm text-gray-600">{(t.treatmentWatchRate * 100).toFixed(2)}%</td>
                    <td className="whitespace-nowrap px-4 py-3 text-sm text-gray-600">{(t.controlWatchRate * 100).toFixed(2)}%</td>
                    <td className="whitespace-nowrap px-4 py-3 text-sm text-gray-600">{formatPValue(t.pValue)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      )}
    </div>
  );
}
