'use client';

import { memo } from 'react';
import {
  BarChart,
  Bar,
  XAxis,
  YAxis,
  CartesianGrid,
  ReferenceLine,
  ResponsiveContainer,
  Tooltip,
  Cell,
} from 'recharts';
import type { MetricResult } from '@/lib/types';

interface SequentialBoundaryPlotProps {
  metricResults: MetricResult[];
  overallAlpha: number;
}

function SequentialBoundaryPlotInner({ metricResults, overallAlpha }: SequentialBoundaryPlotProps) {
  const sequentialMetrics = metricResults.filter((m) => m.sequentialResult);

  if (sequentialMetrics.length === 0) return null;

  const data = sequentialMetrics.map((m) => ({
    metric: m.metricId,
    alphaSpent: m.sequentialResult!.alphaSpent,
    boundaryCrossed: m.sequentialResult!.boundaryCrossed,
    currentLook: m.sequentialResult!.currentLook,
    adjustedPValue: m.sequentialResult!.adjustedPValue,
  }));

  return (
    <section className="mb-6">
      <h3 className="mb-3 text-lg font-semibold text-gray-900">Sequential Testing (Alpha Spending)</h3>
      <div className="rounded-lg border border-gray-200 bg-white p-4">
        <div role="img" aria-label="Bar chart showing alpha spending by metric">
        <ResponsiveContainer width="100%" height={Math.max(200, data.length * 60 + 60)}>
          <BarChart layout="vertical" data={data} margin={{ left: 140, right: 40, top: 10, bottom: 10 }}>
            <CartesianGrid strokeDasharray="3 3" horizontal={false} />
            <XAxis type="number" domain={[0, overallAlpha * 1.2]} />
            <YAxis type="category" dataKey="metric" width={130} tick={{ fontSize: 12 }} />
            <ReferenceLine x={overallAlpha} stroke="#dc2626" strokeDasharray="4 4" label={{ value: `alpha = ${overallAlpha}`, position: 'top', fontSize: 11 }} />
            <Tooltip
              formatter={(value: number) => value.toFixed(4)}
              labelFormatter={(label: string) => `Metric: ${label}`}
            />
            <Bar dataKey="alphaSpent" isAnimationActive={false}>
              {data.map((entry) => (
                <Cell
                  key={entry.metric}
                  fill={entry.boundaryCrossed ? '#dc2626' : '#3b82f6'}
                />
              ))}
            </Bar>
          </BarChart>
        </ResponsiveContainer>
        </div>
        <div className="mt-3 flex flex-wrap gap-3">
          {data.map((d) => (
            <div key={d.metric} className="flex items-center gap-2 text-xs text-gray-600">
              <span className="font-medium">{d.metric}</span>
              <span>Look {d.currentLook}</span>
              {d.boundaryCrossed && (
                <span className="rounded-full bg-red-100 px-2 py-0.5 text-xs font-medium text-red-700">
                  Boundary Crossed
                </span>
              )}
            </div>
          ))}
        </div>
      </div>
    </section>
  );
}

export const SequentialBoundaryPlot = memo(SequentialBoundaryPlotInner);
