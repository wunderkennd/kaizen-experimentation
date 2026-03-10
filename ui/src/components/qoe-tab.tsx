'use client';

import { useEffect, useState } from 'react';
import type { QoeDashboardResult, QoeStatus } from '@/lib/types';
import { getQoeDashboard } from '@/lib/api';

interface QoeTabProps {
  experimentId: string;
}

const STATUS_STYLES: Record<QoeStatus, { bg: string; border: string; dot: string; text: string; label: string }> = {
  GOOD: { bg: 'bg-green-50', border: 'border-green-200', dot: 'bg-green-500', text: 'text-green-800', label: 'Good' },
  WARNING: { bg: 'bg-yellow-50', border: 'border-yellow-200', dot: 'bg-yellow-500', text: 'text-yellow-800', label: 'Warning' },
  CRITICAL: { bg: 'bg-red-50', border: 'border-red-200', dot: 'bg-red-500', text: 'text-red-800', label: 'Critical' },
};

function formatMetricValue(value: number, unit: string): string {
  if (unit === '%') return `${(value * 100).toFixed(2)}%`;
  if (unit === 'ms') return `${value.toFixed(0)} ms`;
  if (unit === 'kbps') return `${value.toFixed(0)} kbps`;
  return `${value.toFixed(1)} ${unit}`;
}

function formatDelta(control: number, treatment: number, lowerIsBetter: boolean, unit: string): { text: string; positive: boolean } {
  const diff = treatment - control;
  const improved = lowerIsBetter ? diff < 0 : diff > 0;
  const absDiff = Math.abs(diff);
  let text: string;
  if (unit === '%') {
    text = `${(absDiff * 100).toFixed(2)}pp`;
  } else if (unit === 'ms') {
    text = `${absDiff.toFixed(0)} ms`;
  } else if (unit === 'kbps') {
    text = `${absDiff.toFixed(0)} kbps`;
  } else {
    text = `${absDiff.toFixed(1)}`;
  }
  return { text: `${improved ? '↓' : '↑'} ${text}`, positive: improved };
}

export function QoeTab({ experimentId }: QoeTabProps) {
  const [result, setResult] = useState<QoeDashboardResult | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    getQoeDashboard(experimentId)
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
        No QoE dashboard available for this experiment.
      </div>
    );
  }

  const overallStyle = STATUS_STYLES[result.overallStatus];

  return (
    <div className="space-y-4">
      {/* Overall status banner */}
      <div className={`rounded-md ${overallStyle.bg} border ${overallStyle.border} p-4`}>
        <div className="flex items-center gap-2">
          <span className={`inline-flex h-3 w-3 rounded-full ${overallStyle.dot}`} aria-hidden="true" />
          <h4 className={`text-sm font-semibold ${overallStyle.text}`}>
            Overall QoE: {overallStyle.label}
          </h4>
        </div>
        <p className={`mt-1 text-sm ${overallStyle.text}`}>
          {result.overallStatus === 'GOOD' && 'All playback quality metrics are within acceptable thresholds.'}
          {result.overallStatus === 'WARNING' && 'Some metrics are approaching threshold limits. Review flagged metrics below.'}
          {result.overallStatus === 'CRITICAL' && 'One or more metrics have breached critical thresholds. Immediate review recommended.'}
        </p>
      </div>

      {/* Metric cards grid */}
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3">
        {result.snapshots.map((snapshot) => {
          const style = STATUS_STYLES[snapshot.status];
          const delta = formatDelta(snapshot.controlValue, snapshot.treatmentValue, snapshot.lowerIsBetter, snapshot.unit);
          return (
            <div
              key={snapshot.metricId}
              className={`rounded-lg border ${style.border} ${style.bg} p-4`}
            >
              <div className="flex items-center justify-between">
                <span className="text-xs font-medium uppercase text-gray-500">{snapshot.label}</span>
                <span className={`inline-flex items-center gap-1.5 rounded-full px-2 py-0.5 text-xs font-medium ${style.bg} ${style.text}`}>
                  <span className={`inline-flex h-2 w-2 rounded-full ${style.dot}`} aria-hidden="true" />
                  {style.label}
                </span>
              </div>
              <div className="mt-3 flex items-end justify-between">
                <div>
                  <p className="text-xs text-gray-500">Treatment</p>
                  <p className="text-xl font-bold text-gray-900">
                    {formatMetricValue(snapshot.treatmentValue, snapshot.unit)}
                  </p>
                </div>
                <div className="text-right">
                  <p className="text-xs text-gray-500">Control</p>
                  <p className="text-sm text-gray-600">
                    {formatMetricValue(snapshot.controlValue, snapshot.unit)}
                  </p>
                </div>
              </div>
              <div className="mt-2 border-t border-gray-200 pt-2">
                <span className={`text-sm font-medium ${delta.positive ? 'text-green-700' : 'text-red-700'}`}>
                  {delta.text}
                </span>
                <span className="ml-1 text-xs text-gray-500">
                  {delta.positive ? 'improvement' : 'regression'}
                </span>
              </div>
            </div>
          );
        })}
      </div>

      <p className="text-xs text-gray-400">
        Last computed: {new Date(result.computedAt).toLocaleString()}
      </p>
    </div>
  );
}
