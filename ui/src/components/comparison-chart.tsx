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
  ErrorBar,
} from 'recharts';
import type { Experiment, AnalysisResult } from '@/lib/types';

interface ComparisonEntry {
  experiment: Experiment;
  analysisResult: AnalysisResult;
}

interface ComparisonChartProps {
  entries: ComparisonEntry[];
}

const COLORS = ['#4f46e5', '#059669', '#d97706', '#dc2626'];

function ComparisonChartInner({ entries }: ComparisonChartProps) {
  const data = entries.map((entry, idx) => {
    const primaryMetric = entry.analysisResult.metricResults.find(
      (m) => m.metricId === entry.experiment.primaryMetricId,
    );

    if (!primaryMetric) {
      return {
        name: entry.experiment.name,
        effect: 0,
        errorLow: 0,
        errorHigh: 0,
        fill: COLORS[idx % COLORS.length],
      };
    }

    return {
      name: entry.experiment.name,
      effect: primaryMetric.absoluteEffect,
      errorLow: primaryMetric.absoluteEffect - primaryMetric.ciLower,
      errorHigh: primaryMetric.ciUpper - primaryMetric.absoluteEffect,
      fill: COLORS[idx % COLORS.length],
    };
  });

  return (
    <section className="mb-6">
      <h2 className="mb-3 text-lg font-semibold text-gray-900">Effect Size Comparison</h2>
      <div className="rounded-lg border border-gray-200 bg-white p-4">
        <p className="mb-3 text-xs text-gray-500">
          Primary metric treatment effects with 95% confidence intervals
        </p>
        <div role="img" aria-label="Bar chart comparing treatment effects with error bars" data-testid="comparison-chart">
          <ResponsiveContainer width="100%" height={Math.max(200, entries.length * 80 + 60)}>
            <BarChart
              layout="vertical"
              data={data}
              margin={{ left: 140, right: 40, top: 10, bottom: 10 }}
            >
              <CartesianGrid strokeDasharray="3 3" horizontal={false} />
              <XAxis type="number" domain={['auto', 'auto']} />
              <YAxis type="category" dataKey="name" width={130} tick={{ fontSize: 12 }} />
              <ReferenceLine x={0} stroke="#6b7280" strokeDasharray="4 4" />
              <Tooltip
                formatter={(value: number) => value.toFixed(4)}
                labelFormatter={(label: string) => `Experiment: ${label}`}
              />
              <Bar dataKey="effect" isAnimationActive={false}>
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
              </Bar>
            </BarChart>
          </ResponsiveContainer>
        </div>
        {/* Legend */}
        <div className="mt-3 flex flex-wrap gap-4">
          {entries.map((entry, idx) => (
            <div key={entry.experiment.experimentId} className="flex items-center gap-1.5 text-xs text-gray-600">
              <span
                className="inline-block h-3 w-3 rounded-sm"
                style={{ backgroundColor: COLORS[idx % COLORS.length] }}
              />
              {entry.experiment.name} ({entry.experiment.primaryMetricId})
            </div>
          ))}
        </div>
      </div>
    </section>
  );
}

export const ComparisonChart = memo(ComparisonChartInner);
