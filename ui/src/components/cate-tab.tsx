'use client';

import { useEffect, useState } from 'react';
import {
  ComposedChart, Bar, XAxis, YAxis, CartesianGrid, Tooltip,
  ResponsiveContainer, ReferenceLine, ErrorBar, Cell,
} from 'recharts';
import type { CateAnalysisResult } from '@/lib/types';
import { getCateAnalysis } from '@/lib/api';
import { formatPValue } from '@/lib/utils';

interface CateTabProps {
  experimentId: string;
}

const SEGMENT_LABELS: Record<string, string> = {
  TRIAL: 'Trial',
  NEW: 'New (<30d)',
  ESTABLISHED: 'Established (30-180d)',
  MATURE: 'Mature (>180d)',
  AT_RISK: 'At Risk',
  WINBACK: 'Winback',
};

export function CateTab({ experimentId }: CateTabProps) {
  const [result, setResult] = useState<CateAnalysisResult | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    getCateAnalysis(experimentId)
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
        No lifecycle segment analysis available for this experiment.
      </div>
    );
  }

  // Prepare chart data: per-segment effects with error bars
  const chartData = result.subgroupEffects.map((sg) => ({
    segment: SEGMENT_LABELS[sg.segment] || sg.segment,
    effect: sg.effect,
    errorX: [sg.effect - sg.ciLower, sg.ciUpper - sg.effect] as [number, number],
    isSignificant: sg.isSignificant,
  }));

  return (
    <div className="space-y-4">
      {/* Heterogeneity banner */}
      {result.heterogeneity.heterogeneityDetected ? (
        <div className="rounded-md bg-amber-50 border border-amber-200 p-4">
          <h4 className="text-sm font-semibold text-amber-800">Heterogeneous Treatment Effects Detected</h4>
          <p className="mt-1 text-sm text-amber-700">
            Cochran Q = {result.heterogeneity.qStatistic.toFixed(1)},
            p = {formatPValue(result.heterogeneity.pValue)},
            I&sup2; = {result.heterogeneity.iSquared.toFixed(1)}%.
            Treatment effects vary significantly across lifecycle segments.
          </p>
        </div>
      ) : (
        <div className="rounded-md bg-green-50 border border-green-200 p-4">
          <h4 className="text-sm font-semibold text-green-800">Homogeneous Treatment Effects</h4>
          <p className="mt-1 text-sm text-green-700">
            Cochran Q = {result.heterogeneity.qStatistic.toFixed(1)},
            p = {formatPValue(result.heterogeneity.pValue)}.
            No significant variation in treatment effects across segments.
          </p>
        </div>
      )}

      {/* Global ATE summary */}
      <div className="grid grid-cols-2 gap-4 sm:grid-cols-4">
        <div className="rounded-lg border border-gray-200 bg-white p-3">
          <span className="text-xs font-medium uppercase text-gray-500">Global ATE</span>
          <p className={`mt-1 text-lg font-semibold ${
            result.globalAte > 0 ? 'text-green-700' : result.globalAte < 0 ? 'text-red-700' : 'text-gray-700'
          }`}>
            {result.globalAte > 0 ? '+' : ''}{result.globalAte.toFixed(4)}
          </p>
        </div>
        <div className="rounded-lg border border-gray-200 bg-white p-3">
          <span className="text-xs font-medium uppercase text-gray-500">95% CI</span>
          <p className="mt-1 text-lg font-semibold text-gray-900">
            [{result.globalCiLower.toFixed(4)}, {result.globalCiUpper.toFixed(4)}]
          </p>
        </div>
        <div className="rounded-lg border border-gray-200 bg-white p-3">
          <span className="text-xs font-medium uppercase text-gray-500">p-value</span>
          <p className="mt-1 text-lg font-semibold text-gray-900">
            {formatPValue(result.globalPValue)}
          </p>
        </div>
        <div className="rounded-lg border border-gray-200 bg-white p-3">
          <span className="text-xs font-medium uppercase text-gray-500">I&sup2;</span>
          <p className={`mt-1 text-lg font-semibold ${
            result.heterogeneity.iSquared > 50 ? 'text-amber-700' : 'text-gray-700'
          }`}>
            {result.heterogeneity.iSquared.toFixed(1)}%
          </p>
        </div>
      </div>

      {/* Forest plot */}
      <div className="rounded-lg border border-gray-200 bg-white p-4">
        <h4 className="mb-3 text-sm font-semibold text-gray-900">Lifecycle Segment Forest Plot</h4>
        <div style={{ width: '100%', height: 40 + chartData.length * 50 }} role="img" aria-label="Forest plot showing treatment effects by lifecycle segment">
          <ResponsiveContainer>
            <ComposedChart
              layout="vertical"
              data={chartData}
              margin={{ top: 5, right: 30, bottom: 5, left: 120 }}
            >
              <CartesianGrid strokeDasharray="3 3" stroke="#e5e7eb" horizontal={false} />
              <XAxis type="number" tick={{ fontSize: 12 }} />
              <YAxis
                type="category"
                dataKey="segment"
                tick={{ fontSize: 12 }}
                width={110}
              />
              <Tooltip
                formatter={(value: number) => value.toFixed(4)}
                labelFormatter={(label: string) => `Segment: ${label}`}
              />
              <ReferenceLine x={0} stroke="#9ca3af" strokeDasharray="3 3" />
              <ReferenceLine
                x={result.globalAte}
                stroke="#6366f1"
                strokeDasharray="5 5"
                label={{ value: 'Global ATE', fill: '#6366f1', fontSize: 10 }}
              />
              <Bar dataKey="effect" barSize={12}>
                <ErrorBar
                  dataKey="errorX"
                  width={4}
                  strokeWidth={1.5}
                  stroke="#374151"
                  direction="x"
                />
                {chartData.map((entry, idx) => (
                  <Cell
                    key={idx}
                    fill={entry.isSignificant ? '#16a34a' : '#9ca3af'}
                  />
                ))}
              </Bar>
            </ComposedChart>
          </ResponsiveContainer>
        </div>
      </div>

      {/* Subgroup effects table */}
      <div className="overflow-x-auto">
        <table className="min-w-full divide-y divide-gray-200">
          <thead>
            <tr className="bg-gray-50">
              <th className="px-4 py-3 text-left text-xs font-medium uppercase text-gray-500">Segment</th>
              <th className="px-4 py-3 text-right text-xs font-medium uppercase text-gray-500">N (ctrl/treat)</th>
              <th className="px-4 py-3 text-right text-xs font-medium uppercase text-gray-500">Control Mean</th>
              <th className="px-4 py-3 text-right text-xs font-medium uppercase text-gray-500">Treatment Mean</th>
              <th className="px-4 py-3 text-right text-xs font-medium uppercase text-gray-500">Effect</th>
              <th className="px-4 py-3 text-right text-xs font-medium uppercase text-gray-500">95% CI</th>
              <th className="px-4 py-3 text-right text-xs font-medium uppercase text-gray-500">p (adj)</th>
              <th className="px-4 py-3 text-center text-xs font-medium uppercase text-gray-500">Sig</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-gray-200 bg-white">
            {result.subgroupEffects.map((sg) => (
              <tr key={sg.segment}>
                <td className="whitespace-nowrap px-4 py-3 text-sm font-medium text-gray-900">
                  {SEGMENT_LABELS[sg.segment] || sg.segment}
                </td>
                <td className="whitespace-nowrap px-4 py-3 text-right text-sm text-gray-600">
                  {sg.nControl.toLocaleString()} / {sg.nTreatment.toLocaleString()}
                </td>
                <td className="whitespace-nowrap px-4 py-3 text-right text-sm font-mono text-gray-700">
                  {sg.controlMean.toFixed(4)}
                </td>
                <td className="whitespace-nowrap px-4 py-3 text-right text-sm font-mono text-gray-700">
                  {sg.treatmentMean.toFixed(4)}
                </td>
                <td className={`whitespace-nowrap px-4 py-3 text-right text-sm font-mono ${
                  sg.effect > 0 ? 'text-green-700' : sg.effect < 0 ? 'text-red-700' : 'text-gray-700'
                }`}>
                  {sg.effect > 0 ? '+' : ''}{sg.effect.toFixed(4)}
                </td>
                <td className="whitespace-nowrap px-4 py-3 text-right text-sm font-mono text-gray-600">
                  [{sg.ciLower.toFixed(4)}, {sg.ciUpper.toFixed(4)}]
                </td>
                <td className="whitespace-nowrap px-4 py-3 text-right text-sm font-mono text-gray-700">
                  {formatPValue(sg.pValueAdjusted)}
                </td>
                <td className="whitespace-nowrap px-4 py-3 text-center">
                  {sg.isSignificant ? (
                    <span className="inline-flex rounded-full bg-green-100 px-2 py-0.5 text-xs font-medium text-green-800">
                      Yes
                    </span>
                  ) : (
                    <span className="inline-flex rounded-full bg-gray-100 px-2 py-0.5 text-xs font-medium text-gray-600">
                      No
                    </span>
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      {/* FDR note */}
      <p className="text-xs text-gray-500">
        p-values adjusted using Benjamini-Hochberg FDR correction (threshold: {result.fdrThreshold}).
        Metric: {result.metricId}.
      </p>
    </div>
  );
}
