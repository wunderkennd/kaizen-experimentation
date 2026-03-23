'use client';

import { memo, useEffect, useState, useCallback } from 'react';
import {
  ComposedChart, Line, Area, XAxis, YAxis, CartesianGrid,
  ReferenceLine, Tooltip, ResponsiveContainer, Legend,
} from 'recharts';
import type { AvlmResult } from '@/lib/types';
import { getAvlmResult, RpcError } from '@/lib/api';

interface AvlmBoundaryPlotProps {
  experimentId: string;
  metricId: string;
}

function AvlmBoundaryPlotInner({ experimentId, metricId }: AvlmBoundaryPlotProps) {
  const [result, setResult] = useState<AvlmResult | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const fetchData = useCallback(() => {
    setLoading(true);
    setError(null);
    getAvlmResult(experimentId, metricId)
      .then(setResult)
      .catch((err) => {
        if (err instanceof RpcError && err.status === 404) {
          setResult(null);
        } else {
          setError(err.message);
        }
      })
      .finally(() => setLoading(false));
  }, [experimentId, metricId]);

  useEffect(() => { fetchData(); }, [fetchData]);

  if (loading) {
    return (
      <div className="flex items-center justify-center py-6" role="status" aria-label="Loading AVLM">
        <div className="h-5 w-5 animate-spin rounded-full border-4 border-gray-300 border-t-indigo-600" />
        <span className="sr-only">Loading</span>
      </div>
    );
  }

  if (error) {
    return (
      <div className="rounded-md bg-red-50 border border-red-200 px-3 py-2 text-xs text-red-700">
        Failed to load AVLM data: {error}
      </div>
    );
  }

  if (!result) return null;

  // Build chart data — confidence band as [lowerBound, upperBound] area
  const chartData = result.boundaryPoints.map((p) => ({
    look: `L${p.look}`,
    informationPct: Math.round(p.informationFraction * 100),
    band: [p.lowerBound, p.upperBound] as [number, number],
    estimate: p.estimate,
    estimateRaw: p.estimateRaw,
  }));

  // Determine if a look crossed the boundary (estimate outside band)
  const crossedLook = result.isConclusive ? result.conclusiveLook : undefined;

  return (
    <div className="rounded-lg border border-gray-200 bg-white p-4">
      <div className="mb-3 flex flex-wrap items-center justify-between gap-2">
        <div>
          <h4 className="text-sm font-semibold text-gray-900">
            AVLM Confidence Sequence — {metricId}
          </h4>
          <p className="text-xs text-gray-500 mt-0.5">
            Variance reduction: {result.varianceReductionPct.toFixed(0)}% (CUPED)
            {' '}· Unifies sequential testing + CUPED
          </p>
        </div>
        <span
          className={`rounded-full px-2 py-0.5 text-xs font-medium ${
            result.isConclusive
              ? 'bg-green-100 text-green-700'
              : 'bg-yellow-100 text-yellow-700'
          }`}
        >
          {result.isConclusive
            ? `Conclusive (Look ${crossedLook})`
            : 'Inconclusive — still running'}
        </span>
      </div>

      {crossedLook && (
        <div className="mb-3 rounded-md bg-green-50 border border-green-200 px-3 py-2">
          <p className="text-xs text-green-800">
            <span className="font-medium">Confidence sequence excluded zero at Look {crossedLook}.</span>
            {' '}Final CUPED estimate: {result.finalEstimate.toFixed(4)}
            {' '}(95% CS: [{result.finalCiLower.toFixed(4)}, {result.finalCiUpper.toFixed(4)}])
          </p>
        </div>
      )}

      <div role="img" aria-label={`AVLM confidence sequence chart for ${metricId}`}>
        <ResponsiveContainer width="100%" height={280}>
          <ComposedChart data={chartData} margin={{ top: 5, right: 30, bottom: 5, left: 30 }}>
            <CartesianGrid strokeDasharray="3 3" stroke="#e5e7eb" />
            <XAxis
              dataKey="look"
              tick={{ fontSize: 12 }}
              label={{ value: 'Look', position: 'insideBottom', offset: -2, fontSize: 12 }}
            />
            <YAxis
              tick={{ fontSize: 11 }}
              label={{ value: 'Effect', angle: -90, position: 'insideLeft', fontSize: 12 }}
              tickFormatter={(v: number) => v.toFixed(3)}
            />
            <ReferenceLine
              y={0}
              stroke="#6b7280"
              strokeDasharray="4 4"
              label={{ value: 'H₀', position: 'right', fontSize: 11, fill: '#6b7280' }}
            />
            <Tooltip
              formatter={(value: number | [number, number], name: string) => {
                if (Array.isArray(value)) {
                  return [`[${value[0].toFixed(4)}, ${value[1].toFixed(4)}]`, 'Confidence sequence'];
                }
                return [
                  value.toFixed(4),
                  name === 'estimate' ? 'CUPED estimate' : 'Raw estimate',
                ];
              }}
              labelFormatter={(label: string) => `Look: ${label}`}
            />
            <Legend
              formatter={(value: string) => {
                if (value === 'band') return 'Confidence sequence (95%)';
                if (value === 'estimate') return 'CUPED estimate';
                return 'Raw estimate';
              }}
            />
            {/* Confidence sequence band */}
            <Area
              type="monotone"
              dataKey="band"
              stroke="#6366f1"
              strokeWidth={1.5}
              fill="#6366f1"
              fillOpacity={0.12}
              name="band"
              isAnimationActive={false}
            />
            {/* CUPED-adjusted point estimate */}
            <Line
              type="monotone"
              dataKey="estimate"
              stroke="#6366f1"
              strokeWidth={2}
              dot={{ r: 4, fill: '#6366f1' }}
              name="estimate"
              isAnimationActive={false}
            />
            {/* Raw unadjusted estimate */}
            <Line
              type="monotone"
              dataKey="estimateRaw"
              stroke="#9ca3af"
              strokeWidth={1.5}
              strokeDasharray="4 2"
              dot={{ r: 3, fill: '#9ca3af' }}
              name="estimateRaw"
              isAnimationActive={false}
            />
          </ComposedChart>
        </ResponsiveContainer>
      </div>

      <div className="mt-2 flex flex-wrap gap-4 text-xs text-gray-500">
        <span>Looks: {result.boundaryPoints.length}</span>
        <span>
          Final: {result.finalEstimate.toFixed(4)}
          {' '}[{result.finalCiLower.toFixed(4)}, {result.finalCiUpper.toFixed(4)}]
        </span>
        <span>Variance reduction: {result.varianceReductionPct.toFixed(0)}%</span>
      </div>
    </div>
  );
}

export const AvlmBoundaryPlot = memo(AvlmBoundaryPlotInner);
