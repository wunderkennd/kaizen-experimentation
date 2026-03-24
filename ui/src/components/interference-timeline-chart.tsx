'use client';

import { memo } from 'react';
import {
  LineChart, Line, XAxis, YAxis, CartesianGrid,
  Tooltip, ResponsiveContainer, ReferenceLine, Legend,
} from 'recharts';
import type { FeedbackLoopResult } from '@/lib/types';

interface InterferenceTimelineChartProps {
  result: FeedbackLoopResult;
}

function InterferenceTimelineChartInner({ result }: InterferenceTimelineChartProps) {
  const chartData = result.prePostComparison.map((p) => ({
    date: p.date.slice(5), // MM-DD
    effect: p.postEffect,
  }));

  if (chartData.length === 0) return null;

  // Retrain dates in MM-DD format, deduplicated
  const retrainDates = [
    ...new Set(result.retrainingEvents.map((e) => e.retrainedAt.slice(5, 10))),
  ];

  return (
    <div className="rounded-lg border border-gray-200 bg-white p-4">
      <h4 className="mb-1 text-sm font-semibold text-gray-900">Treatment Effect Timeline</h4>
      <p className="mb-3 text-xs text-gray-500">
        Treatment effect over time. Orange vertical lines mark model retraining events.
        Abrupt shifts after retraining indicate feedback loop contamination.
      </p>
      <div role="img" aria-label="Treatment effect over time with model retraining events">
        <ResponsiveContainer width="100%" height={240}>
          <LineChart data={chartData} margin={{ top: 5, right: 20, bottom: 5, left: 30 }}>
            <CartesianGrid strokeDasharray="3 3" stroke="#e5e7eb" />
            <XAxis dataKey="date" tick={{ fontSize: 11 }} />
            <YAxis
              tick={{ fontSize: 11 }}
              tickFormatter={(v: number) => v.toFixed(3)}
              label={{ value: 'Effect', angle: -90, position: 'insideLeft', fontSize: 12 }}
            />
            <ReferenceLine y={0} stroke="#9ca3af" strokeDasharray="4 4" />
            {retrainDates.map((d) => (
              <ReferenceLine
                key={d}
                x={d}
                stroke="#f97316"
                strokeWidth={2}
                label={{ value: 'Retrain', position: 'top', fontSize: 9, fill: '#f97316' }}
              />
            ))}
            <Tooltip
              formatter={(value: number) => [value.toFixed(4), 'Treatment effect']}
              labelFormatter={(label: string) => `Date: ${label}`}
            />
            <Legend
              formatter={(value: string) =>
                value === 'effect' ? 'Treatment effect' : value
              }
            />
            <Line
              type="monotone"
              dataKey="effect"
              stroke="#6366f1"
              strokeWidth={2}
              dot={{ r: 3 }}
              isAnimationActive={false}
            />
          </LineChart>
        </ResponsiveContainer>
      </div>
      {result.retrainingEvents.length > 0 && (
        <p className="mt-2 text-xs text-gray-400">
          {result.retrainingEvents.length} retraining event
          {result.retrainingEvents.length !== 1 ? 's' : ''} marked.
        </p>
      )}
    </div>
  );
}

export const InterferenceTimelineChart = memo(InterferenceTimelineChartInner);
