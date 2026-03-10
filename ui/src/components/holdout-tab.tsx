'use client';

import { useEffect, useState } from 'react';
import {
  Line, XAxis, YAxis, CartesianGrid, Tooltip,
  ResponsiveContainer, ReferenceLine, Area, ComposedChart,
} from 'recharts';
import type { CumulativeHoldoutResult } from '@/lib/types';
import { getCumulativeHoldoutResult } from '@/lib/api';

interface HoldoutTabProps {
  experimentId: string;
}

export function HoldoutTab({ experimentId }: HoldoutTabProps) {
  const [result, setResult] = useState<CumulativeHoldoutResult | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    getCumulativeHoldoutResult(experimentId)
      .then(setResult)
      .catch((err) => setError(err.message))
      .finally(() => setLoading(false));
  }, [experimentId]);

  if (loading) {
    return (
      <div className="flex items-center justify-center py-8" role="status" aria-label="Loading">
        <div className="h-6 w-6 animate-spin rounded-full border-4 border-gray-300 border-t-indigo-600" />
        <span className="sr-only">Loading</span>
      </div>
    );
  }

  if (error || !result) {
    return (
      <div className="rounded-md bg-gray-50 p-4 text-sm text-gray-500">
        No cumulative holdout data available for this experiment.
      </div>
    );
  }

  return (
    <div className="space-y-4">
      {/* Status banner */}
      <div className={`rounded-md border p-4 ${
        result.isSignificant
          ? 'bg-green-50 border-green-200'
          : 'bg-gray-50 border-gray-200'
      }`}>
        <h4 className={`text-sm font-semibold ${
          result.isSignificant ? 'text-green-800' : 'text-gray-800'
        }`}>
          Cumulative Holdout — {result.metricId}
        </h4>
        <p className={`mt-1 text-sm ${
          result.isSignificant ? 'text-green-700' : 'text-gray-700'
        }`}>
          {result.isSignificant
            ? 'The cumulative lift is statistically significant. Production changes are delivering measurable impact.'
            : 'The cumulative lift is not yet statistically significant. Continue monitoring.'
          }
        </p>
      </div>

      {/* Key metrics */}
      <div className="grid grid-cols-2 gap-4 sm:grid-cols-3">
        <div className="rounded-lg border border-gray-200 bg-white p-3">
          <span className="text-xs font-medium uppercase text-gray-500">Cumulative Lift</span>
          <p className={`mt-1 text-lg font-semibold ${
            result.currentCumulativeLift > 0 ? 'text-green-700' : 'text-red-700'
          }`}>
            {result.currentCumulativeLift > 0 ? '+' : ''}{result.currentCumulativeLift.toFixed(2)}
          </p>
        </div>
        <div className="rounded-lg border border-gray-200 bg-white p-3">
          <span className="text-xs font-medium uppercase text-gray-500">95% CI</span>
          <p className="mt-1 text-lg font-semibold text-gray-900">
            [{result.currentCiLower.toFixed(2)}, {result.currentCiUpper.toFixed(2)}]
          </p>
        </div>
        <div className="rounded-lg border border-gray-200 bg-white p-3">
          <span className="text-xs font-medium uppercase text-gray-500">Latest Sample</span>
          <p className="mt-1 text-lg font-semibold text-gray-900">
            {result.timeSeries.length > 0
              ? result.timeSeries[result.timeSeries.length - 1].sampleSize.toLocaleString()
              : '—'
            }
          </p>
        </div>
      </div>

      {/* Time series chart */}
      {result.timeSeries.length > 0 && (
        <div className="rounded-lg border border-gray-200 bg-white p-4">
          <h4 className="mb-3 text-sm font-semibold text-gray-900">Cumulative Lift Over Time</h4>
          <div style={{ width: '100%', height: 300 }} role="img" aria-label="Chart showing cumulative lift over time">
            <ResponsiveContainer>
              <ComposedChart data={result.timeSeries} margin={{ top: 5, right: 20, bottom: 5, left: 10 }}>
                <CartesianGrid strokeDasharray="3 3" stroke="#e5e7eb" />
                <XAxis
                  dataKey="date"
                  tickFormatter={(d: string) => new Date(d).toLocaleDateString('en-US', { month: 'short', day: 'numeric' })}
                  tick={{ fontSize: 12 }}
                />
                <YAxis tick={{ fontSize: 12 }} />
                <Tooltip
                  labelFormatter={(d: string) => new Date(d).toLocaleDateString('en-US', { month: 'long', day: 'numeric', year: 'numeric' })}
                  formatter={(value: number, name: string) => [value.toFixed(2), name]}
                />
                <ReferenceLine y={0} stroke="#9ca3af" strokeDasharray="3 3" />
                <Area
                  dataKey="ciUpper"
                  stroke="none"
                  fill="#bbf7d0"
                  fillOpacity={0.4}
                  name="CI Upper"
                />
                <Area
                  dataKey="ciLower"
                  stroke="none"
                  fill="#ffffff"
                  fillOpacity={1}
                  name="CI Lower"
                />
                <Line
                  type="monotone"
                  dataKey="cumulativeLift"
                  stroke="#16a34a"
                  strokeWidth={2}
                  dot={{ fill: '#16a34a', r: 3 }}
                  name="Cumulative Lift"
                />
              </ComposedChart>
            </ResponsiveContainer>
          </div>
        </div>
      )}
    </div>
  );
}
