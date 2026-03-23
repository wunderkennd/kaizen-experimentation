'use client';

import { memo } from 'react';
import {
  AreaChart, Area, XAxis, YAxis, CartesianGrid,
  ReferenceLine, Tooltip, ResponsiveContainer,
} from 'recharts';
import type { AdaptiveNResult } from '@/lib/types';
import { formatDate } from '@/lib/utils';

interface AdaptiveNTimelineProps {
  result: AdaptiveNResult;
}

function AdaptiveNTimelineInner({ result }: AdaptiveNTimelineProps) {
  if (result.zone !== 'PROMISING' || result.timelineProjection.length === 0) return null;

  const chartData = result.timelineProjection.map((p) => ({
    date: p.date,
    n: p.estimatedN,
  }));

  const plannedNLabel = result.plannedN.toLocaleString();
  const recommendedNLabel = result.recommendedN ? result.recommendedN.toLocaleString() : null;

  return (
    <div className="rounded-lg border border-blue-200 bg-blue-50 p-4">
      <div className="mb-3 flex flex-wrap items-start justify-between gap-2">
        <div>
          <h4 className="text-sm font-semibold text-blue-900">
            Extended Timeline — Promising Zone
          </h4>
          <p className="mt-0.5 text-xs text-blue-700">
            Conditional power: {(result.conditionalPower * 100).toFixed(0)}%.
            {result.extensionDays
              ? ` Recommended extension: ${result.extensionDays} days.`
              : null}
            {result.projectedConclusionDate
              ? ` Projected conclusion: ${formatDate(result.projectedConclusionDate)}.`
              : null}
          </p>
        </div>
        {recommendedNLabel && (
          <div className="rounded-md bg-blue-100 border border-blue-200 px-3 py-1.5 text-center">
            <p className="text-xs text-blue-600 font-medium">Recommended N</p>
            <p className="text-sm font-bold text-blue-900">{recommendedNLabel}</p>
            <p className="text-xs text-blue-500">vs planned {plannedNLabel}</p>
          </div>
        )}
      </div>

      <div role="img" aria-label="Projected sample size timeline for promising-zone experiment">
        <ResponsiveContainer width="100%" height={200}>
          <AreaChart data={chartData} margin={{ top: 5, right: 20, bottom: 5, left: 40 }}>
            <CartesianGrid strokeDasharray="3 3" stroke="#bfdbfe" />
            <XAxis
              dataKey="date"
              tick={{ fontSize: 10 }}
              tickFormatter={(d: string) => d.slice(5)} // MM-DD
            />
            <YAxis
              tick={{ fontSize: 10 }}
              tickFormatter={(v: number) => `${(v / 1000).toFixed(0)}k`}
            />
            <ReferenceLine
              y={result.plannedN}
              stroke="#3b82f6"
              strokeDasharray="4 4"
              label={{ value: `Planned N (${plannedNLabel})`, position: 'right', fontSize: 10, fill: '#3b82f6' }}
            />
            {result.recommendedN && (
              <ReferenceLine
                y={result.recommendedN}
                stroke="#1d4ed8"
                strokeDasharray="6 2"
                label={{ value: `Rec. N (${recommendedNLabel})`, position: 'right', fontSize: 10, fill: '#1d4ed8' }}
              />
            )}
            <Tooltip
              formatter={(value: number) => [`${value.toLocaleString()}`, 'Projected N']}
              labelFormatter={(label: string) => `Date: ${label}`}
            />
            <Area
              type="monotone"
              dataKey="n"
              stroke="#3b82f6"
              strokeWidth={2}
              fill="#bfdbfe"
              fillOpacity={0.5}
              isAnimationActive={false}
            />
          </AreaChart>
        </ResponsiveContainer>
      </div>

      <div className="mt-3 flex flex-wrap gap-4 text-xs text-blue-700">
        <span>Current N: {result.currentN.toLocaleString()}</span>
        <span>Planned N: {plannedNLabel}</span>
        {recommendedNLabel && <span>Recommended N: {recommendedNLabel}</span>}
        {result.extensionDays && <span>Extension: +{result.extensionDays} days</span>}
      </div>
    </div>
  );
}

export const AdaptiveNTimeline = memo(AdaptiveNTimelineInner);
