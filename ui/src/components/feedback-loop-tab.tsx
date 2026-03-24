'use client';

import { useEffect, useState, useCallback } from 'react';
import {
  ComposedChart, Line, Bar, BarChart, XAxis, YAxis, CartesianGrid,
  Tooltip, ResponsiveContainer, Legend, ReferenceLine,
} from 'recharts';
import type { FeedbackLoopResult, MitigationSeverity } from '@/lib/types';
import { getFeedbackLoopAnalysis, RpcError } from '@/lib/api';
import { RetryableError } from '@/components/retryable-error';
import { InterferenceTimelineChart } from '@/components/interference-timeline-chart';
import { formatDate } from '@/lib/utils';

interface FeedbackLoopTabProps {
  experimentId: string;
}

const SEVERITY_CONFIG: Record<MitigationSeverity, { bg: string; text: string; border: string; label: string }> = {
  HIGH:   { bg: 'bg-red-50',    text: 'text-red-800',    border: 'border-red-200',    label: 'High' },
  MEDIUM: { bg: 'bg-yellow-50', text: 'text-yellow-800', border: 'border-yellow-200', label: 'Medium' },
  LOW:    { bg: 'bg-gray-50',   text: 'text-gray-700',   border: 'border-gray-200',   label: 'Low' },
};

export function FeedbackLoopTab({ experimentId }: FeedbackLoopTabProps) {
  const [result, setResult] = useState<FeedbackLoopResult | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const fetchData = useCallback(() => {
    setLoading(true);
    setError(null);
    getFeedbackLoopAnalysis(experimentId)
      .then(setResult)
      .catch((err) => {
        if (err instanceof RpcError && err.status === 404) {
          setResult(null);
        } else {
          setError(err.message);
        }
      })
      .finally(() => setLoading(false));
  }, [experimentId]);

  useEffect(() => { fetchData(); }, [fetchData]);

  if (loading) {
    return (
      <div className="flex items-center justify-center py-8" role="status" aria-label="Loading">
        <div className="h-6 w-6 animate-spin rounded-full border-4 border-gray-300 border-t-indigo-600" />
        <span className="sr-only">Loading</span>
      </div>
    );
  }

  if (error) {
    return <RetryableError message={error} onRetry={fetchData} context="feedback loop analysis" />;
  }

  if (!result) {
    return (
      <div className="rounded-md bg-gray-50 p-4 text-sm text-gray-500">
        No feedback loop analysis available for this experiment.
      </div>
    );
  }

  const contaminationPct = (result.contaminationFraction * 100).toFixed(1);
  const biasDelta = (result.rawEstimate - result.biasCorrectedEstimate);
  const biasDeltaPct = result.rawEstimate !== 0
    ? ((biasDelta / Math.abs(result.rawEstimate)) * 100).toFixed(1)
    : '0.0';

  // Pre/post comparison chart data
  const prePostData = result.prePostComparison.map((p) => ({
    date: p.date.slice(5), // MM-DD
    preEffect: p.preEffect,
    postEffect: p.postEffect,
  }));

  // Add retraining event markers for pre/post chart
  const retrainDates = new Set(
    result.retrainingEvents.map((e) => e.retrainedAt.slice(5, 10)),
  );

  // Contamination bar chart data
  const contaminationData = result.contaminationTimeline.map((p) => ({
    date: p.date.slice(5),
    contamination: +(p.contaminationFraction * 100).toFixed(1),
    isRetrain: retrainDates.has(p.date.slice(5)),
  }));

  return (
    <div className="space-y-6">
      {/* Summary cards */}
      <div className="grid grid-cols-2 gap-4 sm:grid-cols-4">
        <div className="rounded-lg border border-gray-200 bg-white p-3">
          <dt className="text-xs font-medium uppercase text-gray-500">Retrain Events</dt>
          <dd className="mt-1 text-2xl font-bold text-gray-900">{result.retrainingEvents.length}</dd>
        </div>
        <div className="rounded-lg border border-gray-200 bg-white p-3">
          <dt className="text-xs font-medium uppercase text-gray-500">Contamination</dt>
          <dd className={`mt-1 text-2xl font-bold ${result.contaminationFraction > 0.2 ? 'text-red-600' : result.contaminationFraction > 0.1 ? 'text-yellow-600' : 'text-green-600'}`}>
            {contaminationPct}%
          </dd>
        </div>
        <div className="rounded-lg border border-gray-200 bg-white p-3">
          <dt className="text-xs font-medium uppercase text-gray-500">Raw Estimate</dt>
          <dd className="mt-1 text-2xl font-bold text-gray-900">{result.rawEstimate.toFixed(4)}</dd>
        </div>
        <div className="rounded-lg border border-gray-200 bg-white p-3">
          <dt className="text-xs font-medium uppercase text-gray-500">Bias-Corrected</dt>
          <dd className="mt-1 text-2xl font-bold text-indigo-700">{result.biasCorrectedEstimate.toFixed(4)}</dd>
          <p className="text-xs text-gray-500 mt-0.5">
            {Math.abs(parseFloat(biasDeltaPct))}% {biasDelta > 0 ? 'deflation' : 'inflation'} removed
          </p>
        </div>
      </div>

      {/* Bias-corrected estimate highlight */}
      <div className="rounded-lg border border-indigo-200 bg-indigo-50 p-4">
        <h4 className="text-sm font-semibold text-indigo-900">Bias-Corrected Estimate</h4>
        <div className="mt-2 flex flex-wrap items-baseline gap-4">
          <div>
            <span className="text-3xl font-bold text-indigo-700">
              {result.biasCorrectedEstimate.toFixed(4)}
            </span>
            <span className="ml-2 text-sm text-indigo-600">
              95% CI [{result.biasCorrectedCiLower.toFixed(4)}, {result.biasCorrectedCiUpper.toFixed(4)}]
            </span>
          </div>
          <div className="text-sm text-indigo-700">
            <span className="font-medium">Raw:</span> {result.rawEstimate.toFixed(4)}
            <span className="ml-2 text-indigo-500">
              (bias: {biasDelta > 0 ? '+' : ''}{biasDelta.toFixed(4)} / {biasDeltaPct}%)
            </span>
          </div>
        </div>
        <p className="mt-2 text-xs text-indigo-600">
          Doubly-robust correction applied using {result.retrainingEvents.length} retraining event
          {result.retrainingEvents.length !== 1 ? 's' : ''} with {contaminationPct}% contamination.
        </p>
      </div>

      {/* Retraining timeline */}
      <div>
        <h4 className="mb-3 text-sm font-semibold text-gray-900">Retraining Timeline</h4>
        <div className="overflow-hidden rounded-lg border border-gray-200 bg-white">
          <table className="min-w-full divide-y divide-gray-200">
            <thead className="bg-gray-50">
              <tr>
                <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">Event ID</th>
                <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">Retrained At</th>
                <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">Model Version</th>
                <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">Trigger</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-gray-200">
              {result.retrainingEvents.map((ev) => (
                <tr key={ev.eventId}>
                  <td className="whitespace-nowrap px-4 py-3 text-sm font-mono text-gray-600">{ev.eventId}</td>
                  <td className="whitespace-nowrap px-4 py-3 text-sm text-gray-700">{formatDate(ev.retrainedAt)}</td>
                  <td className="whitespace-nowrap px-4 py-3 text-sm font-mono text-indigo-700">{ev.modelVersion}</td>
                  <td className="whitespace-nowrap px-4 py-3 text-sm text-gray-600">{ev.triggerReason}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </div>

      {/* Treatment effect timeline with retrain markers */}
      <InterferenceTimelineChart result={result} />

      {/* Pre/post comparison chart */}
      <div className="rounded-lg border border-gray-200 bg-white p-4">
        <h4 className="mb-1 text-sm font-semibold text-gray-900">Pre/Post Comparison — Effect Estimate</h4>
        <p className="mb-3 text-xs text-gray-500">
          Estimated treatment effect before and after each retraining event. Divergence indicates feedback contamination.
        </p>
        <div role="img" aria-label="Pre/post effect comparison chart">
          <ResponsiveContainer width="100%" height={240}>
            <ComposedChart data={prePostData} margin={{ top: 5, right: 20, bottom: 5, left: 30 }}>
              <CartesianGrid strokeDasharray="3 3" stroke="#e5e7eb" />
              <XAxis dataKey="date" tick={{ fontSize: 11 }} />
              <YAxis
                tick={{ fontSize: 11 }}
                tickFormatter={(v: number) => v.toFixed(3)}
                label={{ value: 'Effect', angle: -90, position: 'insideLeft', fontSize: 12 }}
              />
              <ReferenceLine y={0} stroke="#9ca3af" strokeDasharray="4 4" />
              <Tooltip
                formatter={(value: number, name: string) => [
                  value.toFixed(4),
                  name === 'preEffect' ? 'Pre-retrain effect' : 'Post-retrain effect',
                ]}
              />
              <Legend
                formatter={(value: string) =>
                  value === 'preEffect' ? 'Pre-retrain' : 'Post-retrain'
                }
              />
              <Line
                type="monotone"
                dataKey="preEffect"
                stroke="#6b7280"
                strokeWidth={2}
                strokeDasharray="5 3"
                dot={{ r: 3 }}
                isAnimationActive={false}
              />
              <Line
                type="monotone"
                dataKey="postEffect"
                stroke="#6366f1"
                strokeWidth={2}
                dot={{ r: 3 }}
                isAnimationActive={false}
              />
            </ComposedChart>
          </ResponsiveContainer>
        </div>
      </div>

      {/* Contamination chart */}
      <div className="rounded-lg border border-gray-200 bg-white p-4">
        <h4 className="mb-1 text-sm font-semibold text-gray-900">Contamination Over Time</h4>
        <p className="mb-3 text-xs text-gray-500">
          Fraction of users whose treatment assignment was influenced by the retrained model.
          Spikes on retrain days are expected.
        </p>
        <div role="img" aria-label="Contamination fraction over time chart">
          <ResponsiveContainer width="100%" height={200}>
            <BarChart data={contaminationData} margin={{ top: 5, right: 20, bottom: 5, left: 30 }}>
              <CartesianGrid strokeDasharray="3 3" stroke="#e5e7eb" />
              <XAxis dataKey="date" tick={{ fontSize: 11 }} />
              <YAxis
                tick={{ fontSize: 11 }}
                tickFormatter={(v: number) => `${v}%`}
                domain={[0, 'auto']}
                label={{ value: '%', angle: -90, position: 'insideLeft', fontSize: 12 }}
              />
              <ReferenceLine
                y={20}
                stroke="#ef4444"
                strokeDasharray="4 4"
                label={{ value: '20% threshold', position: 'right', fontSize: 10, fill: '#ef4444' }}
              />
              <Tooltip formatter={(value: number) => [`${value}%`, 'Contamination']} />
              <Bar
                dataKey="contamination"
                name="Contamination"
                fill="#f97316"
                fillOpacity={0.7}
                isAnimationActive={false}
              />
            </BarChart>
          </ResponsiveContainer>
        </div>
      </div>

      {/* Mitigation recommendation matrix */}
      <div>
        <h4 className="mb-3 text-sm font-semibold text-gray-900">Mitigation Recommendations</h4>
        <div className="space-y-3">
          {result.recommendations.map((rec) => {
            const cfg = SEVERITY_CONFIG[rec.severity];
            return (
              <div
                key={rec.recommendationId}
                className={`rounded-lg border p-4 ${cfg.bg} ${cfg.border}`}
              >
                <div className="flex items-start gap-3">
                  <span
                    className={`mt-0.5 rounded px-1.5 py-0.5 text-xs font-medium ${cfg.bg} ${cfg.text} border ${cfg.border}`}
                  >
                    {cfg.label}
                  </span>
                  <div className="flex-1">
                    <h5 className={`text-sm font-semibold ${cfg.text}`}>{rec.title}</h5>
                    <p className={`mt-1 text-sm ${cfg.text} opacity-80`}>{rec.description}</p>
                    <div className="mt-2 rounded-md bg-white bg-opacity-60 px-3 py-2">
                      <span className="text-xs font-medium text-gray-600">Action: </span>
                      <span className="text-xs text-gray-700">{rec.action}</span>
                    </div>
                  </div>
                </div>
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}
