'use client';

import { memo, useEffect, useState } from 'react';
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
import { getSlateOpe } from '@/lib/api';
import type { SlateOpeResult } from '@/lib/types';

interface SlatePositionBiasChartProps {
  experimentId: string;
}

const POSITION_COLOR = '#4f46e5';

export const SlatePositionBiasChart = memo(function SlatePositionBiasChart({
  experimentId,
}: SlatePositionBiasChartProps) {
  const [result, setResult] = useState<SlateOpeResult | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setLoading(true);
    setError(null);
    getSlateOpe(experimentId)
      .then(setResult)
      .catch((err) => setError(err.message))
      .finally(() => setLoading(false));
  }, [experimentId]);

  if (loading) {
    return (
      <div className="flex h-40 items-center justify-center" role="status" aria-label="Loading position bias">
        <div className="h-6 w-6 animate-spin rounded-full border-4 border-gray-300 border-t-indigo-600" />
        <span className="sr-only">Loading</span>
      </div>
    );
  }

  if (error || !result) {
    return (
      <p className="text-sm text-gray-500">
        No LIPS OPE data available for this experiment.
      </p>
    );
  }

  const chartData = result.positionBias.map((p) => ({
    position: `Pos ${p.position}`,
    ctr: p.ctr,
    lipsWeight: p.lipsWeight,
  }));

  return (
    <div data-testid="slate-position-bias-chart">
      <div className="mb-2 flex items-center justify-between">
        <h3 className="text-sm font-semibold text-gray-900">Per-Position CTR (LIPS OPE)</h3>
        <span className="text-xs text-gray-500">
          Policy value: {result.estimatedValue.toFixed(4)}
        </span>
      </div>
      <div role="img" aria-label="Per-position click-through rates from LIPS OPE estimate">
        <ResponsiveContainer width="100%" height={220}>
          <BarChart data={chartData} margin={{ left: 10, right: 10, top: 10, bottom: 5 }}>
            <CartesianGrid strokeDasharray="3 3" vertical={false} />
            <XAxis dataKey="position" tick={{ fontSize: 11 }} />
            <YAxis
              tickFormatter={(v: number) => `${(v * 100).toFixed(0)}%`}
              domain={[0, 'auto']}
            />
            <Tooltip
              formatter={(v: number) => `${(v * 100).toFixed(2)}%`}
              labelFormatter={(label) => `${label}`}
            />
            <Bar dataKey="ctr" isAnimationActive={false} name="CTR">
              {chartData.map((_, i) => (
                <Cell key={`cell-${i}`} fill={POSITION_COLOR} fillOpacity={1 - i * 0.06} />
              ))}
            </Bar>
          </BarChart>
        </ResponsiveContainer>
      </div>
      <p className="mt-1 text-xs text-gray-500">
        Bars show click-through rate at each slate position, importance-weighted by LIPS abstraction.
      </p>
    </div>
  );
});
