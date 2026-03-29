'use client';

import { useEffect, useState, useCallback } from 'react';
import type { SwitchbackAnalysisResult } from '@/lib/types';
import { getSwitchbackAnalysis, RpcError } from '@/lib/api';
import { formatPValue } from '@/lib/utils';
import { RetryableError } from '@/components/retryable-error';

interface SwitchbackTabProps {
  experimentId: string;
}

type CarryoverStatus = 'pass' | 'warn' | 'fail';

function getCarryoverStatus(pValue: number, detected: boolean): CarryoverStatus {
  if (detected || pValue < 0.01) return 'fail';
  if (pValue < 0.05) return 'warn';
  return 'pass';
}

const CARRYOVER_CONFIG: Record<CarryoverStatus, { bg: string; text: string; border: string; label: string; description: string }> = {
  pass: {
    bg: 'bg-green-50',
    text: 'text-green-800',
    border: 'border-green-200',
    label: 'Pass',
    description: 'No significant carryover detected. Washout period appears adequate.',
  },
  warn: {
    bg: 'bg-yellow-50',
    text: 'text-yellow-800',
    border: 'border-yellow-200',
    label: 'Warning',
    description: 'Marginal carryover signal detected. Consider extending washout period in future designs.',
  },
  fail: {
    bg: 'bg-red-50',
    text: 'text-red-800',
    border: 'border-red-200',
    label: 'Fail',
    description: 'Significant carryover detected. Treatment effects may be biased. Consider extending washout period or using a longer block duration.',
  },
};

export function SwitchbackTab({ experimentId }: SwitchbackTabProps) {
  const [result, setResult] = useState<SwitchbackAnalysisResult | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const fetchData = useCallback(() => {
    setLoading(true);
    setError(null);
    getSwitchbackAnalysis(experimentId)
      .then(setResult)
      .catch((err) => {
        if (err instanceof RpcError && err.status === 404) return;
        setError(err.message);
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
    return <RetryableError message={error} onRetry={fetchData} context="switchback analysis" />;
  }

  if (!result) {
    return (
      <div className="rounded-md bg-gray-50 p-4 text-sm text-gray-500">
        No switchback analysis available for this experiment.
      </div>
    );
  }

  const carryoverStatus = getCarryoverStatus(result.carryoverPValue, result.carryoverDetected);
  const carryoverCfg = CARRYOVER_CONFIG[carryoverStatus];
  const ciIncludesZero = result.ciLower <= 0 && result.ciUpper >= 0;

  return (
    <div className="space-y-6">
      {/* Treatment effect highlight */}
      <div className="rounded-lg border border-indigo-200 bg-indigo-50 p-4">
        <h4 className="text-sm font-semibold text-indigo-900">Treatment Effect (HAC)</h4>
        <div className="mt-2 flex flex-wrap items-baseline gap-4">
          <div>
            <span className="text-3xl font-bold text-indigo-700">
              {result.treatmentEffect > 0 ? '+' : ''}{result.treatmentEffect.toFixed(4)}
            </span>
            <span className="ml-2 text-sm text-indigo-600">
              95% CI [{result.ciLower.toFixed(4)}, {result.ciUpper.toFixed(4)}]
            </span>
          </div>
          {ciIncludesZero && (
            <span className="rounded-full bg-yellow-100 px-2 py-0.5 text-xs font-medium text-yellow-800">
              CI includes zero
            </span>
          )}
        </div>
        <p className="mt-2 text-xs text-indigo-600">
          HAC (Newey-West) standard error: {result.hacSe.toFixed(4)}
        </p>
      </div>

      {/* Summary cards */}
      <div className="grid grid-cols-2 gap-4 sm:grid-cols-4">
        <div className="rounded-lg border border-gray-200 bg-white p-3">
          <dt className="text-xs font-medium uppercase text-gray-500">HAC p-value</dt>
          <dd className={`mt-1 text-2xl font-bold ${result.hacPValue < 0.05 ? 'text-green-600' : 'text-gray-900'}`}>
            {formatPValue(result.hacPValue)}
          </dd>
        </div>
        <div className="rounded-lg border border-gray-200 bg-white p-3">
          <dt className="text-xs font-medium uppercase text-gray-500">RI p-value</dt>
          <dd className={`mt-1 text-2xl font-bold ${result.riPValue < 0.05 ? 'text-green-600' : 'text-gray-900'}`}>
            {formatPValue(result.riPValue)}
          </dd>
          <p className="text-xs text-gray-400 mt-0.5">Randomization inference</p>
        </div>
        <div className="rounded-lg border border-gray-200 bg-white p-3">
          <dt className="text-xs font-medium uppercase text-gray-500">HAC SE</dt>
          <dd className="mt-1 text-2xl font-bold text-gray-900">
            {result.hacSe.toFixed(4)}
          </dd>
        </div>
        <div className="rounded-lg border border-gray-200 bg-white p-3">
          <dt className="text-xs font-medium uppercase text-gray-500">Carryover p-value</dt>
          <dd className={`mt-1 text-2xl font-bold ${result.carryoverDetected ? 'text-red-600' : 'text-gray-900'}`}>
            {formatPValue(result.carryoverPValue)}
          </dd>
        </div>
      </div>

      {/* Carryover diagnostic badge */}
      <div className={`rounded-lg border p-4 ${carryoverCfg.bg} ${carryoverCfg.border}`}>
        <div className="flex items-start gap-3">
          <span
            className={`mt-0.5 rounded px-2 py-0.5 text-xs font-semibold ${carryoverCfg.bg} ${carryoverCfg.text} border ${carryoverCfg.border}`}
          >
            Carryover: {carryoverCfg.label}
          </span>
          <div className="flex-1">
            <p className={`text-sm ${carryoverCfg.text}`}>
              {carryoverCfg.description}
            </p>
            <p className={`mt-1 text-xs ${carryoverCfg.text} opacity-70`}>
              Lag-1 autocorrelation test p-value: {formatPValue(result.carryoverPValue)}
            </p>
          </div>
        </div>
      </div>

      {/* Inference comparison table */}
      <div>
        <h4 className="mb-3 text-sm font-semibold text-gray-900">Inference Methods Comparison</h4>
        <div className="overflow-hidden rounded-lg border border-gray-200 bg-white">
          <table className="min-w-full divide-y divide-gray-200">
            <thead className="bg-gray-50">
              <tr>
                <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">Method</th>
                <th className="px-4 py-3 text-right text-xs font-medium uppercase tracking-wider text-gray-500">p-value</th>
                <th className="px-4 py-3 text-center text-xs font-medium uppercase tracking-wider text-gray-500">Significant</th>
                <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">Description</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-gray-200">
              <tr>
                <td className="whitespace-nowrap px-4 py-3 text-sm font-medium text-gray-900">HAC (Newey-West)</td>
                <td className="whitespace-nowrap px-4 py-3 text-right text-sm font-mono text-gray-700">{formatPValue(result.hacPValue)}</td>
                <td className="whitespace-nowrap px-4 py-3 text-center">
                  <span className={`inline-flex rounded-full px-2 py-0.5 text-xs font-medium ${result.hacPValue < 0.05 ? 'bg-green-100 text-green-800' : 'bg-gray-100 text-gray-600'}`}>
                    {result.hacPValue < 0.05 ? 'Yes' : 'No'}
                  </span>
                </td>
                <td className="px-4 py-3 text-sm text-gray-600">Robust to autocorrelation in residuals</td>
              </tr>
              <tr>
                <td className="whitespace-nowrap px-4 py-3 text-sm font-medium text-gray-900">Randomization Inference</td>
                <td className="whitespace-nowrap px-4 py-3 text-right text-sm font-mono text-gray-700">{formatPValue(result.riPValue)}</td>
                <td className="whitespace-nowrap px-4 py-3 text-center">
                  <span className={`inline-flex rounded-full px-2 py-0.5 text-xs font-medium ${result.riPValue < 0.05 ? 'bg-green-100 text-green-800' : 'bg-gray-100 text-gray-600'}`}>
                    {result.riPValue < 0.05 ? 'Yes' : 'No'}
                  </span>
                </td>
                <td className="px-4 py-3 text-sm text-gray-600">Exact or Monte Carlo permutation test</td>
              </tr>
            </tbody>
          </table>
        </div>
      </div>

      {/* Methodology note */}
      <div className="rounded-lg border border-gray-200 bg-white p-4">
        <h4 className="text-sm font-semibold text-gray-900">Methodology</h4>
        <p className="mt-2 text-xs text-gray-500">
          Switchback experiments alternate treatment and control across time blocks.
          The treatment effect is estimated using the DoorDash sandwich estimator with
          HAC (Newey-West) standard errors to account for temporal autocorrelation.
          Randomization inference provides a distribution-free alternative p-value.
          The carryover diagnostic tests lag-1 autocorrelation in block-level residuals;
          significant carryover (p &lt; 0.05) suggests the washout period is insufficient.
        </p>
      </div>
    </div>
  );
}
