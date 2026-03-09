'use client';

import { useEffect, useState } from 'react';
import {
  ComposedChart, Line, Scatter, XAxis, YAxis, CartesianGrid,
  Tooltip, ReferenceLine, ResponsiveContainer, Legend,
} from 'recharts';
import type { NoveltyAnalysisResult } from '@/lib/types';
import { getNoveltyAnalysis } from '@/lib/api';
import { formatEffect } from '@/lib/utils';

interface NoveltyTabProps {
  experimentId: string;
}

export function NoveltyTab({ experimentId }: NoveltyTabProps) {
  const [result, setResult] = useState<NoveltyAnalysisResult | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    getNoveltyAnalysis(experimentId)
      .then(setResult)
      .catch((err) => setError(err.message))
      .finally(() => setLoading(false));
  }, [experimentId]);

  if (loading) {
    return (
      <div className="flex items-center justify-center py-8">
        <div className="h-6 w-6 animate-spin rounded-full border-4 border-gray-300 border-t-indigo-600" />
      </div>
    );
  }

  if (error || !result) {
    return (
      <div className="rounded-md bg-gray-50 p-4 text-sm text-gray-500">
        No novelty analysis available for this experiment.
      </div>
    );
  }

  return (
    <div className="space-y-4">
      {/* Detection banner */}
      {result.noveltyDetected ? (
        <div className="rounded-md bg-amber-50 border border-amber-200 p-4">
          <h4 className="text-sm font-semibold text-amber-800">Novelty Effect Detected</h4>
          <p className="mt-1 text-sm text-amber-700">
            The treatment effect for <span className="font-medium">{result.metricId}</span> shows
            an exponential decay pattern. The current effect may overestimate the long-term impact.
          </p>
        </div>
      ) : (
        <div className="rounded-md bg-green-50 border border-green-200 p-4">
          <h4 className="text-sm font-semibold text-green-800">No Novelty Effect</h4>
          <p className="mt-1 text-sm text-green-700">
            Treatment effect appears stable over time.
          </p>
        </div>
      )}

      {/* Key metrics grid */}
      <div className="grid grid-cols-2 gap-4 sm:grid-cols-4">
        <div className="rounded-lg border border-gray-200 bg-white p-3">
          <span className="text-xs font-medium uppercase text-gray-500">Current Effect</span>
          <p className="mt-1 text-lg font-semibold text-gray-900">
            {formatEffect(result.rawTreatmentEffect)}
          </p>
        </div>
        <div className="rounded-lg border border-gray-200 bg-white p-3">
          <span className="text-xs font-medium uppercase text-gray-500">Steady-State Projection</span>
          <p className="mt-1 text-lg font-semibold text-gray-900">
            {formatEffect(result.projectedSteadyStateEffect)}
          </p>
        </div>
        <div className="rounded-lg border border-gray-200 bg-white p-3">
          <span className="text-xs font-medium uppercase text-gray-500">Novelty Amplitude</span>
          <p className="mt-1 text-lg font-semibold text-gray-900">
            {formatEffect(result.noveltyAmplitude)}
          </p>
        </div>
        <div className="rounded-lg border border-gray-200 bg-white p-3">
          <span className="text-xs font-medium uppercase text-gray-500">Decay Constant</span>
          <p className="mt-1 text-lg font-semibold text-gray-900">
            {result.decayConstantDays.toFixed(1)} days
          </p>
        </div>
      </div>

      {/* Decay Curve Chart */}
      {result.dailyEffects && result.dailyEffects.length > 0 && (
        <div className="rounded-lg border border-gray-200 bg-white p-4">
          <h4 className="mb-3 text-sm font-semibold text-gray-900">Treatment Effect Over Time</h4>
          <ResponsiveContainer width="100%" height={300}>
            <ComposedChart data={result.dailyEffects} margin={{ top: 5, right: 20, bottom: 5, left: 20 }}>
              <CartesianGrid strokeDasharray="3 3" stroke="#e5e7eb" />
              <XAxis
                dataKey="day"
                label={{ value: 'Day', position: 'insideBottom', offset: -2 }}
                tick={{ fontSize: 12 }}
              />
              <YAxis
                tickFormatter={(v: number) => v.toFixed(3)}
                tick={{ fontSize: 12 }}
                label={{ value: 'Effect', angle: -90, position: 'insideLeft' }}
              />
              <Tooltip
                formatter={(value: number, name: string) => [
                  value.toFixed(4),
                  name === 'observedEffect' ? 'Observed' : 'Fitted Decay',
                ]}
                labelFormatter={(label: number) => `Day ${label}`}
              />
              <Legend
                formatter={(value: string) =>
                  value === 'observedEffect' ? 'Observed Effect' :
                  value === 'fittedEffect' ? 'Fitted Decay Curve' : value
                }
              />
              <ReferenceLine
                y={result.projectedSteadyStateEffect}
                stroke="#6366f1"
                strokeDasharray="6 3"
                label={{ value: 'Steady State', position: 'right', fontSize: 11, fill: '#6366f1' }}
              />
              <Line
                type="monotone"
                dataKey="fittedEffect"
                stroke="#f59e0b"
                strokeWidth={2}
                dot={false}
                name="fittedEffect"
              />
              <Scatter
                dataKey="observedEffect"
                fill="#3b82f6"
                name="observedEffect"
              />
            </ComposedChart>
          </ResponsiveContainer>
        </div>
      )}

      {/* Stability status */}
      <div className="rounded-lg border border-gray-200 bg-white p-4">
        <h4 className="text-sm font-semibold text-gray-900">Stability Status</h4>
        <div className="mt-2 flex items-center gap-3">
          {result.isStabilized ? (
            <>
              <span className="inline-flex h-2.5 w-2.5 rounded-full bg-green-500" />
              <span className="text-sm text-gray-700">Effect has stabilized</span>
            </>
          ) : (
            <>
              <span className="inline-flex h-2.5 w-2.5 animate-pulse rounded-full bg-amber-500" />
              <span className="text-sm text-gray-700">
                ~{result.daysUntilProjectedStability} days until projected stability
              </span>
            </>
          )}
        </div>
        <p className="mt-2 text-xs text-gray-500">
          Metric: {result.metricId}
        </p>
      </div>
    </div>
  );
}
