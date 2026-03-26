'use client';

import { useEffect, useState, useCallback } from 'react';
import { useParams, useRouter } from 'next/navigation';
import Link from 'next/link';
import type { Flag, FlagType, ExperimentType } from '@/lib/types';
import { getFlag, promoteToExperiment } from '@/lib/api';
import { RetryableError } from '@/components/retryable-error';
import { CopyButton } from '@/components/copy-button';
import { AuthProvider, useAuth } from '@/lib/auth-context';
import { NavHeader } from '@/components/nav-header';

const FLAG_TYPE_BADGE: Record<FlagType, string> = {
  BOOLEAN: 'bg-blue-100 text-blue-800',
  STRING: 'bg-green-100 text-green-800',
  NUMERIC: 'bg-purple-100 text-purple-800',
  JSON: 'bg-orange-100 text-orange-800',
};

function FlagDetailContent() {
  const params = useParams();
  const router = useRouter();
  const { canAtLeast } = useAuth();
  const flagId = params.id as string;

  const [flag, setFlag] = useState<Flag | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const [showPromote, setShowPromote] = useState(false);
  const [expType, setExpType] = useState<ExperimentType>('AB');
  const [primaryMetricId, setPrimaryMetricId] = useState('');
  const [promoting, setPromoting] = useState(false);
  const [promoteError, setPromoteError] = useState<string | null>(null);

  const fetchData = useCallback(() => {
    setLoading(true);
    setError(null);
    getFlag(flagId)
      .then((data) => setFlag(data))
      .catch((err) => setError(err.message))
      .finally(() => setLoading(false));
  }, [flagId]);

  useEffect(() => { fetchData(); }, [fetchData]);

  const handlePromote = async () => {
    if (!primaryMetricId.trim()) return;
    setPromoting(true);
    setPromoteError(null);
    try {
      const experiment = await promoteToExperiment(flagId, expType, primaryMetricId.trim());
      router.push(`/experiments/${experiment.experimentId}`);
    } catch (err) {
      setPromoteError(err instanceof Error ? err.message : 'Promotion failed');
    } finally {
      setPromoting(false);
    }
  };

  if (loading) {
    return (
      <div className="flex items-center justify-center py-12" role="status" aria-label="Loading">
        <div className="h-8 w-8 animate-spin rounded-full border-4 border-gray-300 border-t-indigo-600" />
        <span className="sr-only">Loading</span>
      </div>
    );
  }

  if (error) {
    return <RetryableError message={error} onRetry={fetchData} context="flag detail" />;
  }

  if (!flag) return null;

  const rolloutPct = (flag.rolloutPercentage * 100).toFixed(0);

  return (
    <div>
      <div className="mb-2">
        <Link href="/flags" className="text-sm text-indigo-600 hover:text-indigo-800" data-testid="back-link">
          &larr; Back to Flags
        </Link>
      </div>

      <div className="mb-6 flex items-center gap-3">
        <h1 className="text-2xl font-bold text-gray-900" data-testid="flag-name">{flag.name}</h1>
        <span className={`inline-flex rounded-full px-2 py-0.5 text-xs font-medium ${FLAG_TYPE_BADGE[flag.type] || 'bg-gray-100 text-gray-800'}`}>
          {flag.type}
        </span>
        <span className={`inline-flex rounded-full px-2 py-0.5 text-xs font-medium ${flag.enabled ? 'bg-green-100 text-green-800' : 'bg-gray-100 text-gray-600'}`}>
          {flag.enabled ? 'Enabled' : 'Disabled'}
        </span>
        {canAtLeast('experimenter') && (
          <Link
            href={`/flags/${flag.flagId}/edit`}
            className="rounded-md border border-gray-300 px-3 py-1.5 text-sm font-medium text-gray-700 hover:bg-gray-50"
            data-testid="edit-flag-link"
          >
            Edit
          </Link>
        )}
      </div>

      <div className="mb-6 rounded-lg border border-gray-200 bg-white p-6 shadow-sm">
        <dl className="grid grid-cols-2 gap-x-8 gap-y-4 text-sm">
          <div>
            <dt className="font-medium text-gray-500">Flag ID</dt>
            <dd className="flex items-center gap-2 text-gray-900">
              <code className="text-xs">{flag.flagId}</code>
              <CopyButton text={flag.flagId} label="Copy flag ID" className="h-5 w-5" />
            </dd>
          </div>
          <div>
            <dt className="font-medium text-gray-500">Description</dt>
            <dd className="text-gray-900">{flag.description || '—'}</dd>
          </div>
          <div>
            <dt className="font-medium text-gray-500">Default Value</dt>
            <dd className="text-gray-900"><code className="text-xs">{flag.defaultValue}</code></dd>
          </div>
          <div>
            <dt className="font-medium text-gray-500">Rollout Percentage</dt>
            <dd className="text-gray-900">
              <div className="flex items-center gap-2">
                <div className="h-2 w-32 rounded-full bg-gray-200">
                  <div
                    className="h-2 rounded-full bg-indigo-600"
                    style={{ width: `${rolloutPct}%` }}
                    data-testid="rollout-bar"
                  />
                </div>
                <span>{rolloutPct}%</span>
              </div>
            </dd>
          </div>
          {flag.targetingRuleId && (
            <div>
              <dt className="font-medium text-gray-500">Targeting Rule</dt>
              <dd className="text-gray-900"><code className="text-xs">{flag.targetingRuleId}</code></dd>
            </div>
          )}
        </dl>
      </div>

      {flag.variants.length > 0 && (
        <div className="mb-6">
          <h2 className="mb-3 text-lg font-semibold text-gray-900">Variants</h2>
          <div className="overflow-hidden rounded-lg border border-gray-200 bg-white shadow-sm">
            <table className="min-w-full divide-y divide-gray-200">
              <thead className="bg-gray-50">
                <tr>
                  <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">Variant ID</th>
                  <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">Value</th>
                  <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">Traffic Fraction</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-gray-200">
                {flag.variants.map((v) => (
                  <tr key={v.variantId} data-testid={`variant-row-${v.variantId}`}>
                    <td className="px-4 py-3 text-sm"><code>{v.variantId}</code></td>
                    <td className="px-4 py-3 text-sm"><code>{v.value}</code></td>
                    <td className="px-4 py-3 text-sm">{(v.trafficFraction * 100).toFixed(0)}%</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      )}

      {canAtLeast('experimenter') && (
        <div className="rounded-lg border border-gray-200 bg-white p-6 shadow-sm">
          {!showPromote ? (
            <button
              onClick={() => setShowPromote(true)}
              className="rounded-md bg-indigo-600 px-4 py-2 text-sm font-medium text-white hover:bg-indigo-700"
              data-testid="promote-button"
            >
              Promote to Experiment
            </button>
          ) : (
            <div data-testid="promote-form">
              <h3 className="mb-3 text-sm font-semibold text-gray-900">Promote to Experiment</h3>
              <div className="mb-3 flex flex-wrap items-end gap-3">
                <div>
                  <label htmlFor="exp-type" className="mb-1 block text-xs font-medium text-gray-700">Experiment Type</label>
                  <select
                    id="exp-type"
                    value={expType}
                    onChange={(e) => setExpType(e.target.value as ExperimentType)}
                    className="rounded-md border border-gray-300 px-3 py-2 text-sm"
                    data-testid="promote-exp-type"
                  >
                    <option value="AB">A/B</option>
                    <option value="MULTIVARIATE">Multivariate</option>
                  </select>
                </div>
                <div>
                  <label htmlFor="primary-metric" className="mb-1 block text-xs font-medium text-gray-700">Primary Metric ID</label>
                  <input
                    id="primary-metric"
                    type="text"
                    value={primaryMetricId}
                    onChange={(e) => setPrimaryMetricId(e.target.value)}
                    placeholder="e.g. click_through_rate"
                    className="rounded-md border border-gray-300 px-3 py-2 text-sm"
                    data-testid="promote-metric-id"
                  />
                </div>
                <button
                  onClick={handlePromote}
                  disabled={promoting || !primaryMetricId.trim()}
                  className="rounded-md bg-indigo-600 px-4 py-2 text-sm font-medium text-white hover:bg-indigo-700 disabled:opacity-50"
                  data-testid="promote-submit"
                >
                  {promoting ? 'Promoting...' : 'Promote'}
                </button>
                <button
                  onClick={() => setShowPromote(false)}
                  className="rounded-md border border-gray-300 px-4 py-2 text-sm font-medium text-gray-700 hover:bg-gray-50"
                >
                  Cancel
                </button>
              </div>
              {promoteError && (
                <p className="text-sm text-red-600" data-testid="promote-error">{promoteError}</p>
              )}
            </div>
          )}
        </div>
      )}
    </div>
  );
}

export default function FlagDetailPage() {
  return (
    <AuthProvider>
      <div className="min-h-screen bg-gray-50">
        <NavHeader />
        <main className="mx-auto max-w-7xl px-4 py-8 sm:px-6 lg:px-8">
          <FlagDetailContent />
        </main>
      </div>
    </AuthProvider>
  );
}
