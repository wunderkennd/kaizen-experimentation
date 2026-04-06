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
import type { PortfolioLearningRatePoint } from '@/lib/types';

const PALETTE = [
  '#4f46e5', '#10b981', '#f59e0b', '#ef4444',
  '#8b5cf6', '#06b6d4', '#ec4899', '#84cc16',
];

interface LearningRateChartProps {
  data: PortfolioLearningRatePoint[];
}

/**
 * Line chart showing experiment learning rates (convergence speed) over time.
 * Y-axis is the information accumulation rate — higher means faster convergence.
 */
export const LearningRateChart = memo(function LearningRateChart({ data }: LearningRateChartProps) {
  if (data.length === 0) {
    return (
      <div className="rounded-lg border border-gray-200 bg-white p-6" data-testid="learning-rate-chart">
        <h3 className="text-sm font-semibold text-gray-900">Learning Rate</h3>
        <p className="mt-8 text-center text-sm text-gray-500">No learning rate data available.</p>
      </div>
    );
  }

  const experimentNames = [...new Set(data.map((d) => d.experimentName))];
  const byDate = new Map<string, Record<string, number>>();
  for (const point of data) {
    if (!byDate.has(point.date)) byDate.set(point.date, {});
    byDate.get(point.date)![point.experimentName] = point.learningRate;
  }

  const chartData = [...byDate.entries()]
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([date, values]) => ({ date, ...values }));

  return (
    <div className="rounded-lg border border-gray-200 bg-white p-4" data-testid="learning-rate-chart">
      <h3 className="mb-3 text-sm font-semibold text-gray-900">Learning Rate</h3>
      <p className="mb-2 text-xs text-gray-500">Information accumulation rate per experiment</p>
      <div
        role="img"
        aria-label={`Learning rates for ${experimentNames.length} experiments`}
      >
        <ResponsiveContainer width="100%" height={240}>
          <LineChart data={chartData} margin={{ top: 5, right: 20, bottom: 5, left: 0 }}>
            <CartesianGrid strokeDasharray="3 3" stroke="#e5e7eb" />
            <XAxis dataKey="date" tick={{ fontSize: 11 }} />
            <YAxis tick={{ fontSize: 11 }} />
            <Tooltip labelFormatter={(label: string) => `Date: ${label}`} />
            <Legend />
            {experimentNames.map((name, i) => (
              <Line
                key={name}
                type="monotone"
                dataKey={name}
                stroke={PALETTE[i % PALETTE.length]}
                strokeWidth={2}
                strokeDasharray={i % 2 === 0 ? undefined : '5 5'}
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
