'use client';

import { memo } from 'react';
import {
  BarChart,
  Bar,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  Legend,
  ResponsiveContainer,
} from 'recharts';
import type { ArmObjectiveBreakdown, RewardObjective } from '@/lib/types';

const OBJECTIVE_COLORS = [
  '#4f46e5', // indigo
  '#0891b2', // cyan
  '#059669', // emerald
  '#d97706', // amber
  '#dc2626', // red
  '#7c3aed', // violet
  '#0284c7', // sky
  '#65a30d', // lime
];

interface RewardCompositionChartProps {
  breakdowns: ArmObjectiveBreakdown[];
  objectives: RewardObjective[];
}

function RewardCompositionChartInner({ breakdowns, objectives }: RewardCompositionChartProps) {
  if (breakdowns.length === 0 || objectives.length === 0) {
    return (
      <p className="py-4 text-center text-sm text-gray-500">No objective breakdown data available.</p>
    );
  }

  // Build recharts data: one object per arm with one key per objective metricId
  const chartData = breakdowns.map((bd) => {
    const row: Record<string, string | number> = { arm: bd.armName };
    for (const obj of objectives) {
      row[obj.metricId] = bd.objectiveContributions[obj.metricId] ?? 0;
    }
    return row;
  });

  const compositionLabel = objectives.find((o) => o.isPrimary)?.metricId ?? 'Composed';

  return (
    <div>
      <div role="img" aria-label="Multi-objective reward composition per arm">
        <ResponsiveContainer width="100%" height={260}>
          <BarChart data={chartData} margin={{ left: 20, right: 20, top: 10, bottom: 10 }}>
            <CartesianGrid strokeDasharray="3 3" vertical={false} />
            <XAxis dataKey="arm" tick={{ fontSize: 12 }} />
            <YAxis
              domain={[0, 1]}
              tickFormatter={(v: number) => v.toFixed(2)}
              label={{ value: 'Weighted Contribution', angle: -90, position: 'insideLeft', offset: -5, style: { fontSize: 11 } }}
            />
            <Tooltip formatter={(v: number) => v.toFixed(4)} />
            <Legend />
            {objectives.map((obj, i) => (
              <Bar
                key={obj.metricId}
                dataKey={obj.metricId}
                stackId="reward"
                fill={OBJECTIVE_COLORS[i % OBJECTIVE_COLORS.length]}
                name={`${obj.metricId}${obj.isPrimary ? ' (primary)' : ''}`}
                isAnimationActive={false}
              />
            ))}
          </BarChart>
        </ResponsiveContainer>
      </div>
      <p className="mt-2 text-xs text-gray-500">
        Primary objective: <span className="font-medium">{compositionLabel}</span>
        {' · '}
        Weights: {objectives.map((o) => `${o.metricId} ×${o.weight.toFixed(2)}`).join(', ')}
      </p>
    </div>
  );
}

export const RewardCompositionChart = memo(RewardCompositionChartInner);
