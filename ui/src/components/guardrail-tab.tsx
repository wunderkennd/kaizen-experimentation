'use client';

import { useEffect, useState } from 'react';
import type { GuardrailStatusResult } from '@/lib/types';
import { getGuardrailStatus } from '@/lib/api';
import { formatDate } from '@/lib/utils';

interface GuardrailTabProps {
  experimentId: string;
}

export function GuardrailTab({ experimentId }: GuardrailTabProps) {
  const [result, setResult] = useState<GuardrailStatusResult | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    getGuardrailStatus(experimentId)
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

  if (error) {
    return (
      <div className="rounded-md bg-red-50 p-4 text-sm text-red-700">
        Failed to load guardrail status: {error}
      </div>
    );
  }

  if (!result || result.breaches.length === 0) {
    return (
      <div className="space-y-4">
        <div className="rounded-md bg-green-50 border border-green-200 p-4">
          <h4 className="text-sm font-semibold text-green-800">No Guardrail Breaches</h4>
          <p className="mt-1 text-sm text-green-700">
            All guardrail metrics are within acceptable thresholds.
          </p>
        </div>
      </div>
    );
  }

  const hasPause = result.breaches.some((b) => b.action === 'AUTO_PAUSE');

  return (
    <div className="space-y-4">
      {/* Status banner */}
      {result.isPaused ? (
        <div className="rounded-md bg-red-50 border border-red-200 p-4">
          <h4 className="text-sm font-semibold text-red-800">Experiment Auto-Paused</h4>
          <p className="mt-1 text-sm text-red-700">
            This experiment was automatically paused due to guardrail breaches.
          </p>
        </div>
      ) : hasPause ? (
        <div className="rounded-md bg-amber-50 border border-amber-200 p-4">
          <h4 className="text-sm font-semibold text-amber-800">Guardrail Breaches Detected</h4>
          <p className="mt-1 text-sm text-amber-700">
            {result.breaches.length} breach{result.breaches.length !== 1 ? 'es' : ''} recorded.
            Auto-pause was triggered but the experiment has since been resumed.
          </p>
        </div>
      ) : (
        <div className="rounded-md bg-amber-50 border border-amber-200 p-4">
          <h4 className="text-sm font-semibold text-amber-800">Guardrail Alerts</h4>
          <p className="mt-1 text-sm text-amber-700">
            {result.breaches.length} alert{result.breaches.length !== 1 ? 's' : ''} recorded.
            No auto-pause was triggered.
          </p>
        </div>
      )}

      {/* Breach history table */}
      <div className="overflow-x-auto">
        <table className="min-w-full divide-y divide-gray-200">
          <thead>
            <tr className="bg-gray-50">
              <th className="px-4 py-3 text-left text-xs font-medium uppercase text-gray-500">Time</th>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase text-gray-500">Metric</th>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase text-gray-500">Variant</th>
              <th className="px-4 py-3 text-right text-xs font-medium uppercase text-gray-500">Value</th>
              <th className="px-4 py-3 text-right text-xs font-medium uppercase text-gray-500">Threshold</th>
              <th className="px-4 py-3 text-center text-xs font-medium uppercase text-gray-500">Consecutive</th>
              <th className="px-4 py-3 text-center text-xs font-medium uppercase text-gray-500">Action</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-gray-200 bg-white">
            {result.breaches.map((breach, idx) => (
              <tr key={`${breach.metricId}-${breach.detectedAt}-${idx}`}>
                <td className="whitespace-nowrap px-4 py-3 text-sm text-gray-700">
                  {formatDate(breach.detectedAt)}
                  <span className="ml-1 text-xs text-gray-400">
                    {new Date(breach.detectedAt).toLocaleTimeString('en-US', { hour: '2-digit', minute: '2-digit' })}
                  </span>
                </td>
                <td className="whitespace-nowrap px-4 py-3 text-sm font-medium text-gray-900">
                  {breach.metricId}
                </td>
                <td className="whitespace-nowrap px-4 py-3 text-sm text-gray-600">
                  {breach.variantId}
                </td>
                <td className="whitespace-nowrap px-4 py-3 text-right text-sm font-mono text-red-700">
                  {breach.currentValue.toFixed(4)}
                </td>
                <td className="whitespace-nowrap px-4 py-3 text-right text-sm font-mono text-gray-600">
                  {breach.threshold.toFixed(4)}
                </td>
                <td className="whitespace-nowrap px-4 py-3 text-center text-sm text-gray-700">
                  {breach.consecutiveBreachCount}
                </td>
                <td className="whitespace-nowrap px-4 py-3 text-center">
                  <span className={`inline-flex rounded-full px-2 py-0.5 text-xs font-medium ${
                    breach.action === 'AUTO_PAUSE'
                      ? 'bg-red-100 text-red-800'
                      : 'bg-yellow-100 text-yellow-800'
                  }`}>
                    {breach.action === 'AUTO_PAUSE' ? 'Auto-Pause' : 'Alert'}
                  </span>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}
