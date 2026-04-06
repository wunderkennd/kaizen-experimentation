'use client';

import { useEffect, useState, useCallback } from 'react';
import { getMetaExperimentResult } from '@/lib/api';
import type { MetaExperimentResult, MetaVariantResult } from '@/lib/types';
import { RetryableError } from '@/components/retryable-error';
import { TwoLevelIPWBadge } from './TwoLevelIPWBadge';

const BANDIT_LABELS: Record<string, string> = {
  THOMPSON_SAMPLING: 'Thompson Sampling',
  LINEAR_UCB: 'Linear UCB',
  THOMPSON_LINEAR: 'Thompson Linear',
  NEURAL_CONTEXTUAL: 'Neural Contextual',
};

interface MetaResultsPanelProps {
  experimentId: string;
}

function SignificanceBadge({ pValue }: { pValue: number }) {
  const significant = pValue < 0.05;
  return (
    <span
      className={`inline-flex items-center rounded-full px-2 py-0.5 text-xs font-medium ${
        significant
          ? 'bg-green-100 text-green-700'
          : 'bg-gray-100 text-gray-600'
      }`}
      data-testid="significance-badge"
    >
      {significant ? 'Significant' : 'Not sig.'}
    </span>
  );
}

function VariantResultRow({ result }: { result: MetaVariantResult }) {
  return (
    <tr className="hover:bg-gray-50" data-testid={`meta-variant-row-${result.variantId}`}>
      <td className="py-3 pl-4 pr-3 font-medium text-gray-900">{result.variantName}</td>
      <td className="py-3 px-3 text-sm text-gray-600">
        {BANDIT_LABELS[result.banditType] ?? result.banditType}
      </td>
      <td className="py-3 px-3 font-mono text-sm text-gray-700">{result.bestArm}</td>
      <td className="py-3 px-3 text-right font-mono text-sm text-gray-700">
        {(result.bestArmRewardRate * 100).toFixed(1)}%
      </td>
      <td className="py-3 px-3 text-right font-mono text-sm text-gray-700">
        {(result.avgRewardRate * 100).toFixed(1)}%
      </td>
      <td className="py-3 px-3 text-right font-mono text-sm text-gray-700">
        {(result.explorationFraction * 100).toFixed(0)}%
      </td>
      <td className="py-3 px-3 text-right font-mono text-sm text-gray-700">
        {result.ipwEffect >= 0 ? '+' : ''}{result.ipwEffect.toFixed(4)}
        <span className="ml-1 text-gray-400">
          [{result.ipwCiLower.toFixed(4)}, {result.ipwCiUpper.toFixed(4)}]
        </span>
      </td>
    </tr>
  );
}

export function MetaResultsPanel({ experimentId }: MetaResultsPanelProps) {
  const [result, setResult] = useState<MetaExperimentResult | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const fetchData = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const data = await getMetaExperimentResult(experimentId);
      setResult(data);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load meta-experiment results.');
    } finally {
      setLoading(false);
    }
  }, [experimentId]);

  useEffect(() => {
    fetchData();
  }, [fetchData]);

  if (loading && !result) {
    return (
      <div className="flex items-center justify-center py-8" role="status" aria-label="Loading meta results">
        <div className="h-6 w-6 animate-spin rounded-full border-2 border-gray-300 border-t-indigo-600" />
        <span className="sr-only">Loading meta-experiment results</span>
      </div>
    );
  }

  if (error && !result) {
    return <RetryableError message={error} onRetry={fetchData} context="meta-experiment results" />;
  }

  if (!result) return null;

  return (
    <div className="space-y-4" data-testid="meta-results-panel">
      {/* Summary header */}
      <div className="flex items-center justify-between">
        <h3 className="text-base font-semibold text-gray-900">Meta-Experiment Results</h3>
        {result.overallWinner && (
          <span className="rounded-full bg-indigo-100 px-3 py-1 text-sm font-medium text-indigo-700" data-testid="overall-winner">
            Winner: {result.overallWinner}
          </span>
        )}
      </div>

      {/* Cochran Q test for heterogeneity */}
      <div className="flex items-center gap-3 text-sm text-gray-600">
        <span>Cochran&apos;s Q p-value: <span className="font-mono">{result.cochranQPValue.toFixed(4)}</span></span>
        <SignificanceBadge pValue={result.cochranQPValue} />
      </div>

      {/* Per-variant results table */}
      <div className="overflow-x-auto rounded-lg border border-gray-200 bg-white" data-testid="meta-results-table">
        <table className="min-w-full divide-y divide-gray-200">
          <thead className="bg-gray-50">
            <tr>
              <th scope="col" className="py-3 pl-4 pr-3 text-left text-xs font-medium uppercase tracking-wide text-gray-500">Variant</th>
              <th scope="col" className="py-3 px-3 text-left text-xs font-medium uppercase tracking-wide text-gray-500">Policy</th>
              <th scope="col" className="py-3 px-3 text-left text-xs font-medium uppercase tracking-wide text-gray-500">Best Arm</th>
              <th scope="col" className="py-3 px-3 text-right text-xs font-medium uppercase tracking-wide text-gray-500">Best Arm Rate</th>
              <th scope="col" className="py-3 px-3 text-right text-xs font-medium uppercase tracking-wide text-gray-500">Avg Rate</th>
              <th scope="col" className="py-3 px-3 text-right text-xs font-medium uppercase tracking-wide text-gray-500">Explore %</th>
              <th scope="col" className="py-3 px-3 text-right text-xs font-medium uppercase tracking-wide text-gray-500">IPW Effect [95% CI]</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-gray-100 bg-white">
            {result.variantResults.map((vr) => (
              <VariantResultRow key={vr.variantId} result={vr} />
            ))}
          </tbody>
        </table>
      </div>

      {result.computedAt && (
        <p className="text-xs text-gray-400" data-testid="meta-computed-at">
          Computed at: {new Date(result.computedAt).toLocaleString()}
        </p>
      )}
    </div>
  );
}
