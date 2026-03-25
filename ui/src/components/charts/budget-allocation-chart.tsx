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
} from 'recharts';
import type { PortfolioExperiment } from '@/lib/types';

const PALETTE = [
  '#4f46e5', '#10b981', '#f59e0b', '#ef4444',
  '#8b5cf6', '#06b6d4', '#ec4899', '#84cc16',
];

interface BudgetAllocationChartProps {
  experiments: PortfolioExperiment[];
}

/**
 * Stacked horizontal bar chart showing traffic allocation across experiments.
 * Unallocated traffic is shown in light gray.
 * Uses React.memo to skip re-renders when experiment data is stable.
 */
export const BudgetAllocationChart = memo(function BudgetAllocationChart({
  experiments,
}: BudgetAllocationChartProps) {
  if (experiments.length === 0) {
    return (
      <div className="rounded-lg border border-gray-200 bg-white p-6">
        <h3 className="text-sm font-semibold text-gray-900">Traffic Budget Allocation</h3>
        <p className="mt-8 text-center text-sm text-gray-500">No experiments to display.</p>
      </div>
    );
  }

  const totalAllocated = experiments.reduce((sum, e) => sum + e.allocatedTrafficPct, 0);
  const unallocated = Math.max(0, 1 - totalAllocated);

  // Build a single-row stacked bar: each experiment + unallocated remainder
  const chartData: Record<string, number> = {};
  experiments.forEach((exp, i) => {
    chartData[`exp_${i}`] = exp.allocatedTrafficPct;
  });
  if (unallocated > 0.0001) {
    chartData['unallocated'] = unallocated;
  }

  const tooltipFormatter = (value: number, dataKey: string) => {
    if (dataKey === 'unallocated') return [`${(value * 100).toFixed(1)}%`, 'Unallocated'];
    const idx = parseInt(dataKey.replace('exp_', ''), 10);
    const exp = experiments[idx];
    return [`${(value * 100).toFixed(1)}%`, exp?.name ?? dataKey];
  };

  return (
    <div className="rounded-lg border border-gray-200 bg-white p-4" data-testid="budget-allocation-chart">
      <div className="mb-3">
        <h3 className="text-sm font-semibold text-gray-900">Traffic Budget Allocation</h3>
        <p className="text-xs text-gray-500">
          {(totalAllocated * 100).toFixed(1)}% allocated across {experiments.length} experiment{experiments.length !== 1 ? 's' : ''}
        </p>
      </div>

      <div
        role="img"
        aria-label={`Traffic budget: ${(totalAllocated * 100).toFixed(1)}% allocated across ${experiments.length} experiments`}
      >
        <ResponsiveContainer width="100%" height={72}>
          <BarChart
            layout="vertical"
            data={[chartData]}
            margin={{ top: 0, right: 0, bottom: 0, left: 0 }}
            barCategoryGap={0}
          >
            <XAxis type="number" hide domain={[0, 1]} />
            <YAxis type="category" hide dataKey={() => 'budget'} />
            <Tooltip
              cursor={false}
              formatter={tooltipFormatter}
            />
            {experiments.map((_, i) => (
              <Bar
                key={`exp_${i}`}
                dataKey={`exp_${i}`}
                stackId="budget"
                fill={PALETTE[i % PALETTE.length]}
                isAnimationActive={false}
              >
                <Cell fill={PALETTE[i % PALETTE.length]} />
              </Bar>
            ))}
            {unallocated > 0.0001 && (
              <Bar
                dataKey="unallocated"
                stackId="budget"
                fill="#e5e7eb"
                isAnimationActive={false}
              >
                <Cell fill="#e5e7eb" />
              </Bar>
            )}
          </BarChart>
        </ResponsiveContainer>
      </div>

      {/* Legend */}
      <div className="mt-3 flex flex-wrap gap-3" aria-hidden="true">
        {experiments.map((exp, i) => (
          <span key={exp.experimentId} className="flex items-center gap-1.5 text-xs text-gray-700">
            <span
              className="inline-block h-3 w-3 rounded-sm"
              style={{ backgroundColor: PALETTE[i % PALETTE.length] }}
            />
            {exp.name} ({(exp.allocatedTrafficPct * 100).toFixed(1)}%)
          </span>
        ))}
        {unallocated > 0.0001 && (
          <span className="flex items-center gap-1.5 text-xs text-gray-500">
            <span className="inline-block h-3 w-3 rounded-sm bg-gray-200" />
            Unallocated ({(unallocated * 100).toFixed(1)}%)
          </span>
        )}
      </div>
    </div>
  );
});
