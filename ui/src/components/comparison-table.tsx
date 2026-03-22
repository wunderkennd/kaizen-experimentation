'use client';

import { memo } from 'react';
import type { Experiment, AnalysisResult, MetricResult } from '@/lib/types';
import { formatEffect, formatPValue, formatDate, STATE_CONFIG, TYPE_LABELS } from '@/lib/utils';

interface ComparisonEntry {
  experiment: Experiment;
  analysisResult: AnalysisResult;
}

interface ComparisonTableProps {
  entries: ComparisonEntry[];
}

/** Compute experiment duration in days from startedAt to concludedAt (or now). */
function getDurationDays(exp: Experiment): string {
  if (!exp.startedAt) return '--';
  const start = new Date(exp.startedAt).getTime();
  const end = exp.concludedAt ? new Date(exp.concludedAt).getTime() : Date.now();
  const days = Math.floor((end - start) / (1000 * 60 * 60 * 24));
  return `${days}d`;
}

/** Find the primary metric result for an experiment. */
function getPrimaryMetricResult(entry: ComparisonEntry): MetricResult | undefined {
  return entry.analysisResult.metricResults.find(
    (m) => m.metricId === entry.experiment.primaryMetricId,
  );
}

/** Collect all unique metric IDs across all entries. */
function getAllMetricIds(entries: ComparisonEntry[]): string[] {
  const ids = new Set<string>();
  for (const entry of entries) {
    for (const m of entry.analysisResult.metricResults) {
      ids.add(m.metricId);
    }
  }
  return Array.from(ids);
}

function ComparisonTableInner({ entries }: ComparisonTableProps) {
  const allMetricIds = getAllMetricIds(entries);

  return (
    <div className="space-y-8">
      {/* Metadata comparison */}
      <section>
        <h2 className="mb-3 text-lg font-semibold text-gray-900">Experiment Metadata</h2>
        <div className="overflow-hidden rounded-lg border border-gray-200 bg-white">
          <table className="min-w-full divide-y divide-gray-200" data-testid="metadata-table">
            <thead className="bg-gray-50">
              <tr>
                <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">Property</th>
                {entries.map((e) => (
                  <th key={e.experiment.experimentId} className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">
                    {e.experiment.name}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody className="divide-y divide-gray-200 bg-white">
              <tr>
                <td className="px-4 py-3 text-sm font-medium text-gray-900">Type</td>
                {entries.map((e) => (
                  <td key={e.experiment.experimentId} className="px-4 py-3 text-sm text-gray-600">
                    {TYPE_LABELS[e.experiment.type]}
                  </td>
                ))}
              </tr>
              <tr>
                <td className="px-4 py-3 text-sm font-medium text-gray-900">State</td>
                {entries.map((e) => {
                  const cfg = STATE_CONFIG[e.experiment.state];
                  return (
                    <td key={e.experiment.experimentId} className="px-4 py-3 text-sm">
                      <span className={`inline-flex items-center rounded-full px-2.5 py-0.5 text-xs font-medium ${cfg.bgColor} ${cfg.textColor}`}>
                        {cfg.label}
                      </span>
                    </td>
                  );
                })}
              </tr>
              <tr>
                <td className="px-4 py-3 text-sm font-medium text-gray-900">Owner</td>
                {entries.map((e) => (
                  <td key={e.experiment.experimentId} className="px-4 py-3 text-sm text-gray-600">
                    {e.experiment.ownerEmail}
                  </td>
                ))}
              </tr>
              <tr>
                <td className="px-4 py-3 text-sm font-medium text-gray-900">Duration</td>
                {entries.map((e) => (
                  <td key={e.experiment.experimentId} className="px-4 py-3 text-sm text-gray-600">
                    {getDurationDays(e.experiment)}
                  </td>
                ))}
              </tr>
              <tr>
                <td className="px-4 py-3 text-sm font-medium text-gray-900">Started</td>
                {entries.map((e) => (
                  <td key={e.experiment.experimentId} className="px-4 py-3 text-sm text-gray-600">
                    {e.experiment.startedAt ? formatDate(e.experiment.startedAt) : '--'}
                  </td>
                ))}
              </tr>
              <tr>
                <td className="px-4 py-3 text-sm font-medium text-gray-900">SRM Status</td>
                {entries.map((e) => (
                  <td key={e.experiment.experimentId} className="px-4 py-3 text-sm">
                    {e.analysisResult.srmResult.isMismatch ? (
                      <span className="inline-flex items-center rounded-full bg-red-100 px-2.5 py-0.5 text-xs font-medium text-red-800">
                        Mismatch
                      </span>
                    ) : (
                      <span className="inline-flex items-center rounded-full bg-green-100 px-2.5 py-0.5 text-xs font-medium text-green-800">
                        OK
                      </span>
                    )}
                  </td>
                ))}
              </tr>
              <tr>
                <td className="px-4 py-3 text-sm font-medium text-gray-900">CUPED Variance Reduction</td>
                {entries.map((e) => {
                  const maxReduction = Math.max(...e.analysisResult.metricResults.map((m) => m.varianceReductionPct));
                  return (
                    <td key={e.experiment.experimentId} className="px-4 py-3 text-sm text-gray-600">
                      {maxReduction > 0 ? `${maxReduction}%` : 'N/A'}
                    </td>
                  );
                })}
              </tr>
            </tbody>
          </table>
        </div>
      </section>

      {/* Primary metric comparison */}
      <section>
        <h2 className="mb-3 text-lg font-semibold text-gray-900">Primary Metric Results</h2>
        <div className="overflow-hidden rounded-lg border border-gray-200 bg-white">
          <table className="min-w-full divide-y divide-gray-200" data-testid="primary-metric-table">
            <thead className="bg-gray-50">
              <tr>
                <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">Property</th>
                {entries.map((e) => (
                  <th key={e.experiment.experimentId} className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">
                    {e.experiment.name}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody className="divide-y divide-gray-200 bg-white">
              <tr>
                <td className="px-4 py-3 text-sm font-medium text-gray-900">Primary Metric</td>
                {entries.map((e) => (
                  <td key={e.experiment.experimentId} className="px-4 py-3 text-sm text-gray-600 font-mono">
                    {e.experiment.primaryMetricId}
                  </td>
                ))}
              </tr>
              <tr>
                <td className="px-4 py-3 text-sm font-medium text-gray-900">Treatment Effect</td>
                {entries.map((e) => {
                  const m = getPrimaryMetricResult(e);
                  return (
                    <td key={e.experiment.experimentId} className="px-4 py-3 text-sm text-gray-600">
                      {m ? formatEffect(m.absoluteEffect) : '--'}
                    </td>
                  );
                })}
              </tr>
              <tr>
                <td className="px-4 py-3 text-sm font-medium text-gray-900">95% CI</td>
                {entries.map((e) => {
                  const m = getPrimaryMetricResult(e);
                  return (
                    <td key={e.experiment.experimentId} className="px-4 py-3 text-sm text-gray-600">
                      {m ? `[${formatEffect(m.ciLower)}, ${formatEffect(m.ciUpper)}]` : '--'}
                    </td>
                  );
                })}
              </tr>
              <tr>
                <td className="px-4 py-3 text-sm font-medium text-gray-900">p-value</td>
                {entries.map((e) => {
                  const m = getPrimaryMetricResult(e);
                  return (
                    <td key={e.experiment.experimentId} className="px-4 py-3 text-sm text-gray-600">
                      {m ? formatPValue(m.pValue) : '--'}
                    </td>
                  );
                })}
              </tr>
              <tr>
                <td className="px-4 py-3 text-sm font-medium text-gray-900">Significance</td>
                {entries.map((e) => {
                  const m = getPrimaryMetricResult(e);
                  if (!m) {
                    return <td key={e.experiment.experimentId} className="px-4 py-3 text-sm text-gray-600">--</td>;
                  }
                  return (
                    <td key={e.experiment.experimentId} className="px-4 py-3 text-sm">
                      {m.isSignificant ? (
                        <span className="inline-flex items-center rounded-full bg-green-100 px-2.5 py-0.5 text-xs font-medium text-green-800">
                          Significant
                        </span>
                      ) : (
                        <span className="inline-flex items-center rounded-full bg-gray-100 px-2.5 py-0.5 text-xs font-medium text-gray-600">
                          Not Significant
                        </span>
                      )}
                    </td>
                  );
                })}
              </tr>
            </tbody>
          </table>
        </div>
      </section>

      {/* Metric alignment matrix */}
      <section>
        <h2 className="mb-3 text-lg font-semibold text-gray-900">Metric Alignment Matrix</h2>
        <div className="overflow-hidden rounded-lg border border-gray-200 bg-white">
          <table className="min-w-full divide-y divide-gray-200" data-testid="metric-alignment-table">
            <thead className="bg-gray-50">
              <tr>
                <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">Metric</th>
                {entries.map((e) => (
                  <th key={e.experiment.experimentId} className="px-4 py-3 text-center text-xs font-medium uppercase tracking-wider text-gray-500">
                    {e.experiment.name}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody className="divide-y divide-gray-200 bg-white">
              {allMetricIds.map((metricId) => (
                <tr key={metricId}>
                  <td className="px-4 py-3 text-sm font-medium text-gray-900 font-mono">{metricId}</td>
                  {entries.map((e) => {
                    const hasMetric = e.analysisResult.metricResults.some((m) => m.metricId === metricId);
                    return (
                      <td key={e.experiment.experimentId} className="px-4 py-3 text-center text-sm">
                        {hasMetric ? (
                          <span className="text-green-600" aria-label={`${e.experiment.name} has ${metricId}`}>&#10003;</span>
                        ) : (
                          <span className="text-gray-300" aria-label={`${e.experiment.name} does not have ${metricId}`}>--</span>
                        )}
                      </td>
                    );
                  })}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </section>
    </div>
  );
}

export const ComparisonTable = memo(ComparisonTableInner);
