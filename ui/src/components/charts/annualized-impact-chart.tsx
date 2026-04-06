'use client';

import { memo } from 'react';
import {
  BarChart,
  Bar,
  XAxis,
  YAxis,
  Tooltip,
  ResponsiveContainer,
  Cell,
  CartesianGrid,
  ErrorBar,
} from 'recharts';
import type { AnnualizedImpactEntry } from '@/lib/types';

interface AnnualizedImpactChartProps {
  data: AnnualizedImpactEntry[];
}

/**
 * Horizontal bar chart showing annualized impact per experiment with confidence intervals.
 * Positive impacts are indigo, negative impacts are red.
 */
export const AnnualizedImpactChart = memo(function AnnualizedImpactChart({
  data,
}: AnnualizedImpactChartProps) {
  if (data.length === 0) {
    return (
      <div className="rounded-lg border border-gray-200 bg-white p-6" data-testid="annualized-impact-chart">
        <h3 className="text-sm font-semibold text-gray-900">Annualized Impact</h3>
        <p className="mt-8 text-center text-sm text-gray-500">No annualized impact data available.</p>
      </div>
    );
  }

  const chartData = data.map((entry) => ({
    name: entry.experimentName,
    impact: entry.annualizedImpact,
    errorLower: entry.annualizedImpact - entry.ciLower,
    errorUpper: entry.ciUpper - entry.annualizedImpact,
  }));

  return (
    <div className="rounded-lg border border-gray-200 bg-white p-4" data-testid="annualized-impact-chart">
      <h3 className="mb-3 text-sm font-semibold text-gray-900">Annualized Impact</h3>
      <p className="mb-2 text-xs text-gray-500">Projected yearly effect size with 95% CI</p>
      <div
        role="img"
        aria-label={`Annualized impact for ${data.length} experiments`}
      >
        <ResponsiveContainer width="100%" height={Math.max(200, data.length * 48)}>
          <BarChart
            layout="vertical"
            data={chartData}
            margin={{ top: 5, right: 30, bottom: 5, left: 100 }}
          >
            <CartesianGrid strokeDasharray="3 3" stroke="#e5e7eb" horizontal={false} />
            <XAxis type="number" tick={{ fontSize: 11 }} />
            <YAxis type="category" dataKey="name" tick={{ fontSize: 11 }} width={90} />
            <Tooltip
              formatter={(value: number) => [value.toFixed(4), 'Annualized Impact']}
            />
            <Bar dataKey="impact" isAnimationActive={false}>
              <ErrorBar dataKey="errorUpper" direction="right" width={4} stroke="#6b7280" />
              <ErrorBar dataKey="errorLower" direction="left" width={4} stroke="#6b7280" />
              {chartData.map((entry, i) => (
                <Cell
                  key={i}
                  fill={entry.impact >= 0 ? '#4f46e5' : '#ef4444'}
                />
              ))}
            </Bar>
          </BarChart>
        </ResponsiveContainer>
      </div>
    </div>
  );
});
