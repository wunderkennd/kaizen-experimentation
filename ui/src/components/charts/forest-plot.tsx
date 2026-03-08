'use client';

import {
  ComposedChart,
  Scatter,
  XAxis,
  YAxis,
  CartesianGrid,
  ReferenceLine,
  ResponsiveContainer,
  ErrorBar,
  Tooltip,
} from 'recharts';
import type { MetricResult } from '@/lib/types';

interface ForestPlotProps {
  metricResults: MetricResult[];
  showCuped: boolean;
}

export function ForestPlot({ metricResults, showCuped }: ForestPlotProps) {
  const data = metricResults.map((m) => {
    const useCuped = showCuped && m.varianceReductionPct > 0;
    const effect = useCuped ? m.cupedAdjustedEffect : m.absoluteEffect;
    const ciLow = useCuped ? m.cupedCiLower : m.ciLower;
    const ciHigh = useCuped ? m.cupedCiUpper : m.ciUpper;

    return {
      metric: m.metricId,
      effect,
      errorLow: effect - ciLow,
      errorHigh: ciHigh - effect,
      isSignificant: m.isSignificant,
      fill: m.isSignificant ? '#16a34a' : '#9ca3af',
    };
  });

  return (
    <section className="mb-6">
      <h3 className="mb-3 text-lg font-semibold text-gray-900">Treatment Effects</h3>
      <div className="rounded-lg border border-gray-200 bg-white p-4">
        <ResponsiveContainer width="100%" height={Math.max(200, data.length * 60 + 60)}>
          <ComposedChart layout="vertical" data={data} margin={{ left: 140, right: 40, top: 10, bottom: 10 }}>
            <CartesianGrid strokeDasharray="3 3" horizontal={false} />
            <XAxis type="number" domain={['auto', 'auto']} />
            <YAxis type="category" dataKey="metric" width={130} tick={{ fontSize: 12 }} />
            <ReferenceLine x={0} stroke="#6b7280" strokeDasharray="4 4" />
            <Tooltip
              formatter={(value: number) => value.toFixed(4)}
              labelFormatter={(label: string) => `Metric: ${label}`}
            />
            <Scatter dataKey="effect" shape="circle" isAnimationActive={false}>
              <ErrorBar
                dataKey="errorHigh"
                direction="x"
                width={8}
                strokeWidth={2}
              />
              <ErrorBar
                dataKey="errorLow"
                direction="x"
                width={8}
                strokeWidth={2}
              />
            </Scatter>
          </ComposedChart>
        </ResponsiveContainer>
      </div>
    </section>
  );
}
