'use client';

import { useEffect, useState, useCallback } from 'react';
import type { SyntheticControlAnalysisResult, SyntheticControlMethod } from '@/lib/types';
import { getSyntheticControlAnalysis, RpcError } from '@/lib/api';
import { formatPValue } from '@/lib/utils';
import { RetryableError } from '@/components/retryable-error';

interface SyntheticControlTabProps {
  experimentId: string;
}

const METHOD_LABELS: Record<SyntheticControlMethod, string> = {
  CLASSIC: 'Classic SCM',
  AUGMENTED: 'Augmented SCM',
  SYNTHETIC_DID: 'Synthetic DiD',
  CAUSAL_IMPACT: 'CausalImpact',
};

const METHOD_DESCRIPTIONS: Record<SyntheticControlMethod, string> = {
  CLASSIC: 'Constrained weight optimization (Abadie et al.). Weighted combination of donor units minimizing pre-treatment RMSPE.',
  AUGMENTED: 'Ridge de-biased augmented SCM. Handles extrapolation better when the treated unit lies outside the convex hull of donors.',
  SYNTHETIC_DID: 'Synthetic difference-in-differences (Arkhangelsky et al.). Combines synthetic control with DiD for improved robustness.',
  CAUSAL_IMPACT: 'Bayesian structural time series (Brodersen et al.). Models counterfactual with uncertainty quantification.',
};

type FitQuality = 'good' | 'adequate' | 'poor';

function getFitQuality(rmspe: number): FitQuality {
  if (rmspe <= 0.05) return 'good';
  if (rmspe <= 0.2) return 'adequate';
  return 'poor';
}

const FIT_CONFIG: Record<FitQuality, { bg: string; text: string; label: string }> = {
  good: { bg: 'bg-green-100', text: 'text-green-800', label: 'Good Fit' },
  adequate: { bg: 'bg-yellow-100', text: 'text-yellow-800', label: 'Adequate Fit' },
  poor: { bg: 'bg-red-100', text: 'text-red-800', label: 'Poor Fit' },
};

export function SyntheticControlTab({ experimentId }: SyntheticControlTabProps) {
  const [result, setResult] = useState<SyntheticControlAnalysisResult | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const fetchData = useCallback(() => {
    setLoading(true);
    setError(null);
    getSyntheticControlAnalysis(experimentId)
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
    return <RetryableError message={error} onRetry={fetchData} context="synthetic control analysis" />;
  }

  if (!result) {
    return (
      <div className="rounded-md bg-gray-50 p-4 text-sm text-gray-500">
        No synthetic control analysis available for this experiment.
      </div>
    );
  }

  const fitQuality = getFitQuality(result.preTreatmentRmspe);
  const fitCfg = FIT_CONFIG[fitQuality];
  const ciIncludesZero = result.ciLower <= 0 && result.ciUpper >= 0;

  // Sort donor weights descending
  const sortedDonors = Object.entries(result.donorWeights)
    .sort(([, a], [, b]) => b - a);
  const activeDonors = sortedDonors.filter(([, w]) => w > 0.001);
  const zeroDonors = sortedDonors.filter(([, w]) => w <= 0.001);

  return (
    <div className="space-y-6">
      {/* Method badge */}
      <div className="flex items-center gap-3">
        <span className="rounded-full bg-indigo-100 px-3 py-1 text-xs font-semibold text-indigo-800">
          {METHOD_LABELS[result.method] || result.method}
        </span>
        <span className="text-xs text-gray-500">
          {METHOD_DESCRIPTIONS[result.method] || ''}
        </span>
      </div>

      {/* Treatment effect highlight */}
      <div className="rounded-lg border border-indigo-200 bg-indigo-50 p-4">
        <h4 className="text-sm font-semibold text-indigo-900">Treatment Effect</h4>
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
      </div>

      {/* Summary cards */}
      <div className="grid grid-cols-2 gap-4 sm:grid-cols-3">
        <div className="rounded-lg border border-gray-200 bg-white p-3">
          <dt className="text-xs font-medium uppercase text-gray-500">Permutation p-value</dt>
          <dd className={`mt-1 text-2xl font-bold ${result.permutationPValue < 0.05 ? 'text-green-600' : 'text-gray-900'}`}>
            {formatPValue(result.permutationPValue)}
          </dd>
          <p className="text-xs text-gray-400 mt-0.5">Placebo test</p>
        </div>
        <div className="rounded-lg border border-gray-200 bg-white p-3">
          <dt className="text-xs font-medium uppercase text-gray-500">Pre-treatment RMSPE</dt>
          <dd className="mt-1 text-2xl font-bold text-gray-900">
            {result.preTreatmentRmspe.toFixed(4)}
          </dd>
          <div className="mt-1">
            <span className={`inline-flex rounded-full px-2 py-0.5 text-xs font-medium ${fitCfg.bg} ${fitCfg.text}`}>
              {fitCfg.label}
            </span>
          </div>
        </div>
        <div className="rounded-lg border border-gray-200 bg-white p-3">
          <dt className="text-xs font-medium uppercase text-gray-500">Active Donors</dt>
          <dd className="mt-1 text-2xl font-bold text-gray-900">
            {activeDonors.length}
          </dd>
          <p className="text-xs text-gray-400 mt-0.5">of {sortedDonors.length} total</p>
        </div>
      </div>

      {/* Pre-treatment fit quality warning */}
      {fitQuality === 'poor' && (
        <div className="rounded-md bg-red-50 border border-red-200 p-4">
          <h4 className="text-sm font-semibold text-red-800">Poor Pre-Treatment Fit</h4>
          <p className="mt-1 text-sm text-red-700">
            The RMSPE ({result.preTreatmentRmspe.toFixed(4)}) exceeds 0.2, indicating the synthetic
            control does not adequately reproduce the treated unit in the pre-treatment period.
            Treatment effect estimates may be unreliable. Consider adding more donors or using the
            Augmented SCM method for better extrapolation.
          </p>
        </div>
      )}

      {/* Donor weights table */}
      <div>
        <h4 className="mb-3 text-sm font-semibold text-gray-900">
          Donor Weights ({activeDonors.length} active)
        </h4>
        <div className="overflow-hidden rounded-lg border border-gray-200 bg-white">
          <table className="min-w-full divide-y divide-gray-200">
            <thead className="bg-gray-50">
              <tr>
                <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">Donor Unit</th>
                <th className="px-4 py-3 text-right text-xs font-medium uppercase tracking-wider text-gray-500">Weight</th>
                <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">Contribution</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-gray-200">
              {activeDonors.map(([donorId, weight]) => (
                <tr key={donorId}>
                  <td className="whitespace-nowrap px-4 py-3 text-sm font-medium text-gray-900">{donorId}</td>
                  <td className="whitespace-nowrap px-4 py-3 text-right text-sm font-mono text-gray-700">
                    {weight.toFixed(4)}
                  </td>
                  <td className="px-4 py-3">
                    <div className="flex items-center gap-2">
                      <div className="flex-1 h-3 rounded bg-gray-100 overflow-hidden">
                        <div
                          className="h-full rounded bg-indigo-500"
                          style={{ width: `${Math.min(weight * 100, 100)}%` }}
                        />
                      </div>
                      <span className="w-12 text-right text-xs text-gray-500">
                        {(weight * 100).toFixed(1)}%
                      </span>
                    </div>
                  </td>
                </tr>
              ))}
              {zeroDonors.length > 0 && (
                <tr>
                  <td colSpan={3} className="px-4 py-3 text-sm text-gray-400 italic">
                    {zeroDonors.length} donor{zeroDonors.length !== 1 ? 's' : ''} with negligible weight (&lt; 0.1%)
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
      </div>

      {/* Placebo test results */}
      <div className="rounded-lg border border-gray-200 bg-white p-4">
        <h4 className="text-sm font-semibold text-gray-900">Placebo Test</h4>
        <p className="mt-2 text-sm text-gray-600">
          The permutation p-value ({formatPValue(result.permutationPValue)}) is computed by
          iteratively applying the synthetic control method to each donor unit as if it were treated.
          A significant p-value (&lt; 0.05) indicates the observed treatment effect is larger than
          would be expected by chance.
        </p>
        <div className="mt-3 flex items-center gap-2">
          <span className={`inline-flex rounded-full px-2 py-0.5 text-xs font-medium ${
            result.permutationPValue < 0.05 ? 'bg-green-100 text-green-800' : 'bg-gray-100 text-gray-600'
          }`}>
            {result.permutationPValue < 0.05 ? 'Significant' : 'Not Significant'}
          </span>
          <span className="text-xs text-gray-500">
            at the 0.05 level
          </span>
        </div>
      </div>

      {/* Methodology note */}
      <div className="rounded-lg border border-gray-200 bg-white p-4">
        <h4 className="text-sm font-semibold text-gray-900">Methodology</h4>
        <p className="mt-2 text-xs text-gray-500">
          Synthetic control methods estimate the causal effect of an intervention on a treated unit
          by constructing a weighted combination of untreated donor units that best reproduces
          the treated unit in the pre-treatment period. The treatment effect is the difference
          between the observed outcome and the synthetic counterfactual after treatment.
          Pre-treatment RMSPE measures fit quality; the placebo test validates statistical significance.
        </p>
        <div className="mt-3 flex items-center gap-6 text-xs text-gray-600">
          <span className="flex items-center gap-1.5">
            <span className="inline-block h-2.5 w-2.5 rounded-full bg-green-500" aria-hidden="true" />
            Good (RMSPE &le; 0.05)
          </span>
          <span className="flex items-center gap-1.5">
            <span className="inline-block h-2.5 w-2.5 rounded-full bg-yellow-500" aria-hidden="true" />
            Adequate (0.05 &lt; RMSPE &le; 0.2)
          </span>
          <span className="flex items-center gap-1.5">
            <span className="inline-block h-2.5 w-2.5 rounded-full bg-red-500" aria-hidden="true" />
            Poor (RMSPE &gt; 0.2)
          </span>
        </div>
      </div>
    </div>
  );
}
