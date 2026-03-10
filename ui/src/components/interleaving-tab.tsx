'use client';

import { useEffect, useState } from 'react';
import {
  BarChart,
  Bar,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  ResponsiveContainer,
  Cell,
} from 'recharts';
import type { InterleavingAnalysisResult } from '@/lib/types';
import { getInterleavingAnalysis } from '@/lib/api';
import { formatPValue } from '@/lib/utils';

interface InterleavingTabProps {
  experimentId: string;
}

export function InterleavingTab({ experimentId }: InterleavingTabProps) {
  const [result, setResult] = useState<InterleavingAnalysisResult | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    getInterleavingAnalysis(experimentId)
      .then(setResult)
      .catch((err) => setError(err.message))
      .finally(() => setLoading(false));
  }, [experimentId]);

  if (loading) {
    return (
      <div className="flex items-center justify-center py-8" role="status" aria-label="Loading">
        <div className="h-6 w-6 animate-spin rounded-full border-4 border-gray-300 border-t-indigo-600" />
        <span className="sr-only">Loading</span>
      </div>
    );
  }

  if (error || !result) {
    return (
      <div className="rounded-md bg-gray-50 p-4 text-sm text-gray-500">
        No interleaving analysis available for this experiment.
      </div>
    );
  }

  const winRateData = Object.entries(result.algorithmWinRates).map(([id, rate]) => ({
    algorithm: id,
    winRate: rate,
  }));

  const strengthData = result.algorithmStrengths.map((s) => ({
    algorithm: s.algorithmId,
    strength: s.strength,
    ciLower: s.ciLower,
    ciUpper: s.ciUpper,
  }));

  const algorithms = Object.keys(result.algorithmWinRates);

  return (
    <div className="space-y-4">
      {/* Sign test result */}
      <div className={`rounded-md border p-4 ${
        result.signTestPValue < 0.05
          ? 'bg-green-50 border-green-200'
          : 'bg-gray-50 border-gray-200'
      }`}>
        <h4 className={`text-sm font-semibold ${
          result.signTestPValue < 0.05 ? 'text-green-800' : 'text-gray-800'
        }`}>
          Sign Test: p = {formatPValue(result.signTestPValue)}
        </h4>
        <p className={`mt-1 text-sm ${
          result.signTestPValue < 0.05 ? 'text-green-700' : 'text-gray-600'
        }`}>
          {result.signTestPValue < 0.05
            ? 'Significant difference detected between algorithms.'
            : 'No significant difference between algorithms.'}
        </p>
      </div>

      {/* Win rates chart */}
      <div className="rounded-lg border border-gray-200 bg-white p-4">
        <h4 className="mb-3 text-sm font-semibold text-gray-900">Algorithm Win Rates</h4>
        <div role="img" aria-label="Bar chart showing algorithm win rates">
        <ResponsiveContainer width="100%" height={200}>
          <BarChart data={winRateData} margin={{ left: 20, right: 20, top: 10, bottom: 10 }}>
            <CartesianGrid strokeDasharray="3 3" vertical={false} />
            <XAxis dataKey="algorithm" tick={{ fontSize: 12 }} />
            <YAxis domain={[0, 1]} tickFormatter={(v: number) => `${(v * 100).toFixed(0)}%`} />
            <Tooltip formatter={(v: number) => `${(v * 100).toFixed(1)}%`} />
            <Bar dataKey="winRate" isAnimationActive={false}>
              {winRateData.map((entry, index) => (
                <Cell key={entry.algorithm} fill={index === 0 ? '#9ca3af' : '#4f46e5'} />
              ))}
            </Bar>
          </BarChart>
        </ResponsiveContainer>
        </div>
      </div>

      {/* Bradley-Terry strengths table */}
      <div>
        <h4 className="mb-2 text-sm font-semibold text-gray-900">Bradley-Terry Strength Estimates</h4>
        <div className="overflow-hidden rounded-lg border border-gray-200 bg-white">
          <table className="min-w-full divide-y divide-gray-200">
            <thead className="bg-gray-50">
              <tr>
                <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">Algorithm</th>
                <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">Strength</th>
                <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">95% CI</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-gray-200">
              {strengthData.map((s) => (
                <tr key={s.algorithm}>
                  <td className="whitespace-nowrap px-4 py-3 text-sm font-medium text-gray-900">{s.algorithm}</td>
                  <td className="whitespace-nowrap px-4 py-3 text-sm text-gray-600">{s.strength.toFixed(3)}</td>
                  <td className="whitespace-nowrap px-4 py-3 text-sm text-gray-600">
                    [{s.ciLower.toFixed(3)}, {s.ciUpper.toFixed(3)}]
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </div>

      {/* Position analysis heatmap */}
      {result.positionAnalyses.length > 0 && (
        <div>
          <h4 className="mb-2 text-sm font-semibold text-gray-900">Position Engagement Rates</h4>
          <div className="overflow-hidden rounded-lg border border-gray-200 bg-white">
            <table className="min-w-full divide-y divide-gray-200">
              <thead className="bg-gray-50">
                <tr>
                  <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">Position</th>
                  {algorithms.map((alg) => (
                    <th key={alg} className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">{alg}</th>
                  ))}
                </tr>
              </thead>
              <tbody className="divide-y divide-gray-200">
                {result.positionAnalyses.map((pa) => (
                  <tr key={pa.position}>
                    <td className="whitespace-nowrap px-4 py-3 text-sm font-medium text-gray-900">#{pa.position}</td>
                    {algorithms.map((alg) => {
                      const rate = pa.algorithmEngagementRates[alg] ?? 0;
                      const opacity = Math.max(0.1, rate);
                      return (
                        <td key={alg} className="whitespace-nowrap px-4 py-3 text-sm text-gray-600">
                          <span
                            className="inline-block rounded px-2 py-0.5"
                            style={{ backgroundColor: `rgba(79, 70, 229, ${opacity})`, color: rate > 0.2 ? 'white' : '#374151' }}
                          >
                            {(rate * 100).toFixed(1)}%
                          </span>
                        </td>
                      );
                    })}
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
