'use client';

import Link from 'next/link';
import type { Experiment, AnalysisResult, GuardrailStatusResult } from '@/lib/types';
import { TYPE_LABELS } from '@/lib/utils';

interface MonitoringHealthTableProps {
  experiments: Experiment[];
  analysisResults: Record<string, AnalysisResult>;
  guardrailStatuses: Record<string, GuardrailStatusResult>;
}

function computeDaysRunning(startedAt?: string): number {
  if (!startedAt) return 0;
  const start = new Date(startedAt).getTime();
  const now = Date.now();
  return Math.floor((now - start) / (1000 * 60 * 60 * 24));
}

function PrimaryMetricIndicator({ analysis }: { analysis?: AnalysisResult }) {
  if (!analysis || analysis.metricResults.length === 0) {
    return (
      <span className="inline-flex items-center gap-1 text-sm text-gray-400" data-testid="metric-status-unknown">
        <span className="inline-block h-2 w-2 rounded-full bg-gray-300" aria-hidden="true" />
        No data
      </span>
    );
  }

  const primary = analysis.metricResults[0];
  let color: string;
  let label: string;

  if (primary.isSignificant && primary.absoluteEffect > 0) {
    color = 'bg-green-500';
    label = 'Positive';
  } else if (primary.isSignificant && primary.absoluteEffect < 0) {
    color = 'bg-red-500';
    label = 'Negative';
  } else {
    color = 'bg-yellow-500';
    label = 'Inconclusive';
  }

  return (
    <span className="inline-flex items-center gap-1 text-sm" data-testid="metric-status">
      <span className={`inline-block h-2 w-2 rounded-full ${color}`} aria-hidden="true" />
      {label}
    </span>
  );
}

function GuardrailIndicator({ status }: { status?: GuardrailStatusResult }) {
  if (!status || status.breaches.length === 0) {
    return (
      <span className="inline-flex items-center gap-1 text-sm text-green-700" data-testid="guardrail-ok">
        <span className="inline-block h-2 w-2 rounded-full bg-green-500" aria-hidden="true" />
        OK
      </span>
    );
  }

  const breachCount = status.breaches.length;
  return (
    <span className="inline-flex items-center gap-1 text-sm text-red-700" data-testid="guardrail-breach">
      <span className="inline-block h-2 w-2 rounded-full bg-red-500" aria-hidden="true" />
      {breachCount} {breachCount === 1 ? 'breach' : 'breaches'}
    </span>
  );
}

function SrmBadge({ analysis }: { analysis?: AnalysisResult }) {
  if (!analysis) {
    return <span className="text-sm text-gray-400">--</span>;
  }

  if (analysis.srmResult.isMismatch) {
    return (
      <span
        className="inline-flex items-center rounded-full bg-red-100 px-2 py-0.5 text-xs font-medium text-red-800"
        data-testid="srm-mismatch"
      >
        SRM Mismatch
      </span>
    );
  }

  return (
    <span
      className="inline-flex items-center rounded-full bg-green-100 px-2 py-0.5 text-xs font-medium text-green-800"
      data-testid="srm-ok"
    >
      OK
    </span>
  );
}

export function MonitoringHealthTable({
  experiments,
  analysisResults,
  guardrailStatuses,
}: MonitoringHealthTableProps) {
  const running = experiments.filter((e) => e.state === 'RUNNING');

  if (running.length === 0) {
    return (
      <div className="py-8 text-center" data-testid="no-running-experiments">
        <p className="text-sm text-gray-500">No running experiments.</p>
      </div>
    );
  }

  return (
    <div className="overflow-hidden rounded-lg border border-gray-200 bg-white shadow-sm">
      <table className="min-w-full divide-y divide-gray-200" data-testid="health-table">
        <thead className="bg-gray-50">
          <tr>
            <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">
              Name
            </th>
            <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">
              Owner
            </th>
            <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">
              Type
            </th>
            <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">
              Days Running
            </th>
            <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">
              Primary Metric
            </th>
            <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">
              Guardrails
            </th>
            <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">
              SRM Check
            </th>
          </tr>
        </thead>
        <tbody className="divide-y divide-gray-200">
          {running.map((exp) => {
            const analysis = analysisResults[exp.experimentId];
            const guardrail = guardrailStatuses[exp.experimentId];
            const daysRunning = computeDaysRunning(exp.startedAt);

            return (
              <tr key={exp.experimentId} data-testid={`health-row-${exp.experimentId}`}>
                <td className="whitespace-nowrap px-4 py-3 text-sm">
                  <Link
                    href={`/experiments/${exp.experimentId}`}
                    className="font-medium text-indigo-600 hover:text-indigo-900"
                    data-testid={`experiment-link-${exp.experimentId}`}
                  >
                    {exp.name}
                  </Link>
                </td>
                <td className="whitespace-nowrap px-4 py-3 text-sm text-gray-600">
                  {exp.ownerEmail}
                </td>
                <td className="whitespace-nowrap px-4 py-3 text-sm text-gray-600">
                  {TYPE_LABELS[exp.type]}
                </td>
                <td className="whitespace-nowrap px-4 py-3 text-sm text-gray-900" data-testid={`days-running-${exp.experimentId}`}>
                  {daysRunning}
                </td>
                <td className="whitespace-nowrap px-4 py-3">
                  <PrimaryMetricIndicator analysis={analysis} />
                </td>
                <td className="whitespace-nowrap px-4 py-3">
                  <GuardrailIndicator status={guardrail} />
                </td>
                <td className="whitespace-nowrap px-4 py-3">
                  <SrmBadge analysis={analysis} />
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}
