'use client';

import type { Experiment, GuardrailBreachEvent, GuardrailStatusResult } from '@/lib/types';
import { formatDate } from '@/lib/utils';

interface MonitoringBreachListProps {
  experiments: Experiment[];
  guardrailStatuses: Record<string, GuardrailStatusResult>;
}

interface BreachWithExperiment extends GuardrailBreachEvent {
  experimentName: string;
}

export function MonitoringBreachList({ experiments, guardrailStatuses }: MonitoringBreachListProps) {
  const experimentNameMap: Record<string, string> = {};
  for (const exp of experiments) {
    experimentNameMap[exp.experimentId] = exp.name;
  }

  const allBreaches: BreachWithExperiment[] = [];
  for (const [experimentId, status] of Object.entries(guardrailStatuses)) {
    for (const breach of status.breaches) {
      allBreaches.push({
        ...breach,
        experimentName: experimentNameMap[experimentId] || experimentId,
      });
    }
  }

  // Sort by detectedAt descending (most recent first)
  allBreaches.sort((a, b) => new Date(b.detectedAt).getTime() - new Date(a.detectedAt).getTime());

  if (allBreaches.length === 0) {
    return (
      <div className="py-8 text-center" data-testid="no-breaches">
        <p className="text-sm text-gray-500">No guardrail breaches detected.</p>
      </div>
    );
  }

  return (
    <div className="space-y-3" data-testid="breach-list">
      {allBreaches.map((breach, idx) => (
        <div
          key={`${breach.experimentId}-${breach.metricId}-${breach.detectedAt}-${idx}`}
          className="rounded-lg border border-red-200 bg-red-50 p-4"
          data-testid="breach-item"
        >
          <div className="flex items-start justify-between">
            <div>
              <p className="text-sm font-semibold text-red-800">
                {breach.experimentName}
              </p>
              <p className="mt-1 text-sm text-red-700">
                Metric: <span className="font-medium">{breach.metricId}</span>
                {' '}(variant: {breach.variantId})
              </p>
              <p className="mt-1 text-sm text-red-700">
                Value: <span className="font-medium">{breach.currentValue.toFixed(4)}</span>
                {' '}vs threshold: <span className="font-medium">{breach.threshold.toFixed(4)}</span>
              </p>
              <p className="mt-1 text-sm text-red-700">
                Consecutive breaches: {breach.consecutiveBreachCount}
              </p>
            </div>
            <div className="flex flex-col items-end gap-1">
              <span
                className={`inline-flex items-center rounded-full px-2 py-0.5 text-xs font-medium ${
                  breach.action === 'AUTO_PAUSE'
                    ? 'bg-red-200 text-red-900'
                    : 'bg-yellow-200 text-yellow-900'
                }`}
                data-testid="breach-action"
              >
                {breach.action === 'AUTO_PAUSE' ? 'Auto-Paused' : 'Alert Only'}
              </span>
              <span className="text-xs text-gray-500">
                {formatDate(breach.detectedAt)}
              </span>
            </div>
          </div>
        </div>
      ))}
    </div>
  );
}
