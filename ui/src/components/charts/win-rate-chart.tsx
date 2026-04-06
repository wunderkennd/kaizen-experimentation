'use client';

import { memo } from 'react';
import {
  LineChart,
  Line,
  XAxis,
  YAxis,
  Tooltip,
  ResponsiveContainer,
  Legend,
  CartesianGrid,
} from 'recharts';
import type { PortfolioWinRatePoint } from '@/lib/types';

const PALETTE = [
  '#4f46e5', '#10b981', '#f59e0b', '#ef4444',
  '#8b5cf6', '#06b6d4', '#ec4899', '#84cc16',
];

interface WinRateChartProps {
  data: PortfolioWinRatePoint[];
}

/**
 * Line chart showing experiment win rates over time.
 * Each experiment is a separate line, grouped by date.
 */
export const WinRateChart = memo(function WinRateChart({ data }: WinRateChartProps) {
  if (data.length === 0) {
    return (
      <div className="rounded-lg border border-gray-200 bg-white p-6" data-testid="win-rate-chart">
        <h3 className="text-sm font-semibold text-gray-900">Win Rate Over Time</h3>
        <p className="mt-8 text-center text-sm text-gray-500">No win rate data available.</p>
      </div>
    );
  }

  // Group by date, pivot experiments into columns
  const experimentNames = [...new Set(data.map((d) => d.experimentName))];
  const byDate = new Map<string, Record<string, number>>();
  for (const point of data) {
    if (!byDate.has(point.date)) byDate.set(point.date, {});
    byDate.get(point.date)![point.experimentName] = point.winRate;
  }

  const chartData = [...byDate.entries()]
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([date, values]) => ({ date, ...values }));

  return (
    <div className="rounded-lg border border-gray-200 bg-white p-4" data-testid="win-rate-chart">
      <h3 className="mb-3 text-sm font-semibold text-gray-900">Win Rate Over Time</h3>
      <div
        role="img"
        aria-label={`Win rates for ${experimentNames.length} experiments over time`}
      >
        <ResponsiveContainer width="100%" height={240}>
          <LineChart data={chartData} margin={{ top: 5, right: 20, bottom: 5, left: 0 }}>
            <CartesianGrid strokeDasharray="3 3" stroke="#e5e7eb" />
            <XAxis dataKey="date" tick={{ fontSize: 11 }} />
            <YAxis
              tick={{ fontSize: 11 }}
              tickFormatter={(v: number) => `${(v * 100).toFixed(0)}%`}
              domain={[0, 1]}
            />
            <Tooltip
              formatter={(value: number) => [`${(value * 100).toFixed(1)}%`, undefined]}
              labelFormatter={(label: string) => `Date: ${label}`}
            />
            <Legend />
            {experimentNames.map((name, i) => (
              <Line
                key={name}
                type="monotone"
                dataKey={name}
                stroke={PALETTE[i % PALETTE.length]}
                strokeWidth={2}
                dot={{ r: 3 }}
                isAnimationActive={false}
              />
            ))}
          </LineChart>
        </ResponsiveContainer>
      </div>
    </div>
  );
});
