'use client';

import { useEffect, useState } from 'react';
import {
  ComposedChart, Line, Scatter, XAxis, YAxis, CartesianGrid,
  Tooltip, ResponsiveContainer, Legend,
} from 'recharts';
import type { GstTrajectoryResult } from '@/lib/types';
import { getGstTrajectory } from '@/lib/api';

interface GstTrajectoryChartProps {
  experimentId: string;
  metricId: string;
}

const METHOD_LABELS: Record<string, string> = {
  MSPRT: 'mSPRT',
  GST_OBF: "O'Brien-Fleming",
  GST_POCOCK: 'Pocock',
};

export function GstTrajectoryChart({ experimentId, metricId }: GstTrajectoryChartProps) {
  const [result, setResult] = useState<GstTrajectoryResult | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    getGstTrajectory(experimentId, metricId)
      .then(setResult)
      .catch((err) => setError(err.message))
      .finally(() => setLoading(false));
  }, [experimentId, metricId]);

  if (loading) {
    return (
      <div className="flex items-center justify-center py-6" role="status" aria-label="Loading">
        <div className="h-5 w-5 animate-spin rounded-full border-4 border-gray-300 border-t-indigo-600" />
        <span className="sr-only">Loading</span>
      </div>
    );
  }

  if (error || !result) return null;

  // Determine if boundary was crossed at any look
  const crossedLook = result.boundaryPoints.find(
    (p) => p.observedZScore !== undefined && p.observedZScore >= p.boundaryZScore
  );

  // Chart data: merge boundary and observed z-scores per look
  const chartData = result.boundaryPoints.map((p) => ({
    look: `Look ${p.look}`,
    informationFraction: (p.informationFraction * 100).toFixed(0) + '%',
    boundary: p.boundaryZScore,
    observed: p.observedZScore ?? null,
  }));

  return (
    <div className="rounded-lg border border-gray-200 bg-white p-4">
      <div className="mb-3 flex items-center justify-between">
        <h4 className="text-sm font-semibold text-gray-900">
          Stopping Boundary — {metricId}
        </h4>
        <span className="rounded-full bg-gray-100 px-2 py-0.5 text-xs font-medium text-gray-600">
          {METHOD_LABELS[result.method] || result.method}
        </span>
      </div>

      {crossedLook && (
        <div className="mb-3 rounded-md bg-red-50 border border-red-200 px-3 py-2">
          <p className="text-xs text-red-700">
            <span className="font-medium">Boundary crossed at Look {crossedLook.look}</span>
            {' '}(Z = {crossedLook.observedZScore?.toFixed(2)}, boundary = {crossedLook.boundaryZScore.toFixed(2)})
          </p>
        </div>
      )}

      <div role="img" aria-label="Line chart showing stopping boundary trajectory">
      <ResponsiveContainer width="100%" height={280}>
        <ComposedChart data={chartData} margin={{ top: 5, right: 20, bottom: 5, left: 20 }}>
          <CartesianGrid strokeDasharray="3 3" stroke="#e5e7eb" />
          <XAxis
            dataKey="look"
            tick={{ fontSize: 12 }}
          />
          <YAxis
            label={{ value: 'Z-score', angle: -90, position: 'insideLeft', fontSize: 12 }}
            tick={{ fontSize: 12 }}
          />
          <Tooltip
            formatter={(value: number, name: string) => [
              value?.toFixed(2) ?? '—',
              name === 'boundary' ? 'Rejection Boundary' : 'Observed Z-score',
            ]}
          />
          <Legend
            formatter={(value: string) =>
              value === 'boundary' ? 'Rejection Boundary' : 'Observed Z-score'
            }
          />
          <Line
            type="stepAfter"
            dataKey="boundary"
            stroke="#dc2626"
            strokeWidth={2}
            strokeDasharray="6 3"
            dot={{ r: 4, fill: '#dc2626' }}
            name="boundary"
            connectNulls={false}
          />
          <Scatter
            dataKey="observed"
            fill="#3b82f6"
            name="observed"
          />
        </ComposedChart>
      </ResponsiveContainer>
      </div>

      <div className="mt-2 flex items-center gap-4 text-xs text-gray-500">
        <span>Planned looks: {result.plannedLooks}</span>
        <span>Overall α: {result.overallAlpha}</span>
      </div>
    </div>
  );
}
