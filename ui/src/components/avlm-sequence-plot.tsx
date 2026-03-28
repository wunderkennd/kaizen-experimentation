'use client';

import { memo, useEffect, useState, useCallback } from 'react';
import {
  ComposedChart, Area, Line, XAxis, YAxis, CartesianGrid,
  ReferenceLine, Tooltip, ResponsiveContainer, Legend,
} from 'recharts';
import type { AvlmResult } from '@/lib/types';
import { getAvlmResult, RpcError } from '@/lib/api';

interface AvlmSequencePlotProps {
  experimentId: string;
  metricId: string;
}

interface ChartPoint {
  look: number;
  band: [number, number];
  estimate: number;
}

function AvlmSequencePlotInner({ experimentId, metricId }: AvlmSequencePlotProps) {
  const [result, setResult] = useState<AvlmResult | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const fetchData = useCallback(() => {
    setLoading(true);
    setError(null);
    getAvlmResult(experimentId, metricId)
      .then(setResult)
      .catch((err: unknown) => {
        if (err instanceof RpcError && err.status === 404) {
          setResult(null);
        } else {
          setError(err instanceof Error ? err.message : 'Failed to load AVLM data');
        }
      })
      .finally(() => setLoading(false));
  }, [experimentId, metricId]);

  useEffect(() => { fetchData(); }, [fetchData]);

  if (loading) {
    return (
      <div className="flex items-center justify-center py-6" role="status" aria-label="Loading AVLM sequence">
        <div className="h-5 w-5 animate-spin rounded-full border-4 border-gray-300 border-t-indigo-600" />
        <span className="sr-only">Loading</span>
      </div>
    );
  }

  if (error) {
    return (
      <div className="rounded-md bg-red-50 border border-red-200 px-3 py-2 text-xs text-red-700">
        {error}
      </div>
    );
  }

  if (!result) return null;

  const chartData: ChartPoint[] = result.boundaryPoints.map((p) => ({
    look: p.look,
    band: [p.lowerBound, p.upperBound] as [number, number],
    estimate: p.estimate,
  }));

  return (
    <div className="rounded-lg border border-indigo-100 bg-white p-4">
      <div className="mb-3 flex flex-wrap items-center justify-between gap-2">
        <div>
          <h4 className="text-sm font-semibold text-gray-900">
            Confidence Sequence — {metricId}
          </h4>
          <p className="text-xs text-gray-500 mt-0.5">
            AVLM · Variance reduction: {result.varianceReductionPct.toFixed(0)}%
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
            ? `Conclusive — Look ${result.conclusiveLook ?? '?'}`
            : 'Ongoing'}
        </span>
      </div>

      <div role="img" aria-label={`AVLM confidence sequence plot for ${metricId}`}>
        <ResponsiveContainer width="100%" height={240}>
          <ComposedChart data={chartData} margin={{ top: 8, right: 28, bottom: 8, left: 28 }}>
            <CartesianGrid strokeDasharray="3 3" stroke="#e5e7eb" />
            <XAxis
              dataKey="look"
              type="number"
              tick={{ fontSize: 11 }}
              label={{ value: 'Look', position: 'insideBottom', offset: -2, fontSize: 11 }}
            />
            <YAxis
              tick={{ fontSize: 11 }}
              label={{ value: 'Effect', angle: -90, position: 'insideLeft', fontSize: 11 }}
              tickFormatter={(v: number) => v.toFixed(3)}
            />
            <ReferenceLine
              y={0}
              stroke="#6b7280"
              strokeDasharray="4 4"
              label={{ value: 'H₀', position: 'right', fontSize: 10, fill: '#6b7280' }}
            />
            <Tooltip
              formatter={(
                value: number | string | (number | string)[],
                name: string,
              ) => {
                if (Array.isArray(value)) {
                  const lo = Number(value[0]).toFixed(4);
                  const hi = Number(value[1]).toFixed(4);
                  return [`[${lo}, ${hi}]`, '95% Confidence Sequence'];
                }
                return [Number(value).toFixed(4), name === 'estimate' ? 'CUPED Estimate' : name];
              }}
              labelFormatter={(label: number) => `Look ${label}`}
            />
            <Legend
              formatter={(value: string) =>
                value === 'band' ? '95% Confidence Sequence' : 'CUPED Estimate'
              }
            />
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
            <Line
              type="monotone"
              dataKey="estimate"
              stroke="#6366f1"
              strokeWidth={2}
              dot={{ r: 4, fill: '#6366f1' }}
              name="estimate"
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
      </div>
    </div>
  );
}

export const AvlmSequencePlot = memo(AvlmSequencePlotInner);
