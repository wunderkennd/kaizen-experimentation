'use client';

import { useEffect, useState, useCallback } from 'react';
import { useParams } from 'next/navigation';
import Link from 'next/link';
import type { Experiment, Variant } from '@/lib/types';
import { getExperiment, updateExperiment, startExperiment, concludeExperiment, archiveExperiment, isPermissionDenied } from '@/lib/api';
import { formatDate } from '@/lib/utils';
import { useAuth } from '@/lib/auth-context';
import { useToast } from '@/lib/toast-context';
import { RetryableError } from '@/components/retryable-error';
import { Breadcrumb } from '@/components/breadcrumb';
import { StateBadge } from '@/components/state-badge';
import { TypeBadge } from '@/components/type-badge';
import { VariantTable } from '@/components/variant-table';
import { VariantForm } from '@/components/variant-form';
import { StateActions } from '@/components/state-actions';
import { StartingChecklist } from '@/components/starting-checklist';
import { ConcludingProgress } from '@/components/concluding-progress';
import { LayerAllocationChart } from '@/components/layer-allocation-chart';
import { AdaptiveNBadge } from '@/components/adaptive-n-badge';
import { MetaExperimentConfig } from '@/components/meta/MetaExperimentConfig';
import { TwoLevelIPWBadge } from '@/components/meta/TwoLevelIPWBadge';

export default function ExperimentDetailPage() {
  const params = useParams<{ id: string }>();
  const [experiment, setExperiment] = useState<Experiment | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const { canAtLeast } = useAuth();
  const { addToast } = useToast();
  const canEdit = canAtLeast('experimenter');

  const fetchData = useCallback(() => {
    if (!params.id) return;
    setLoading(true);
    setError(null);
    getExperiment(params.id)
      .then(setExperiment)
      .catch((err) => setError(err.message))
      .finally(() => setLoading(false));
  }, [params.id]);

  useEffect(() => { fetchData(); }, [fetchData]);

  const handleSaveVariants = useCallback(async (variants: Variant[]) => {
    if (!experiment) return;
    const updated = await updateExperiment({ ...experiment, variants });
    setExperiment(updated);
  }, [experiment]);

  const handleTransition = useCallback(async (action: 'start' | 'conclude' | 'archive') => {
    if (!experiment) return;
    const id = experiment.experimentId;
    try {
      let updated: Experiment;
      switch (action) {
        case 'start':
          updated = await startExperiment(id);
          break;
        case 'conclude':
          updated = await concludeExperiment(id);
          break;
        case 'archive':
          updated = await archiveExperiment(id);
          break;
      }
      setExperiment(updated);
      addToast(`Experiment ${action === 'start' ? 'started' : action === 'conclude' ? 'concluded' : 'archived'} successfully`, 'success');
    } catch (err) {
      if (isPermissionDenied(err)) {
        addToast('Permission denied: your role does not allow this action.', 'error');
      } else {
        addToast(err instanceof Error ? err.message : 'Transition failed', 'error');
      }
    }
  }, [experiment, addToast]);

  const handleCopyId = useCallback(() => {
    if (!experiment) return;
    navigator.clipboard.writeText(experiment.experimentId);
    addToast('Experiment ID copied to clipboard', 'success');
  }, [experiment, addToast]);

  if (loading) {
    return (
      <div className="flex items-center justify-center py-12" role="status" aria-label="Loading">
        <div className="h-8 w-8 animate-spin rounded-full border-4 border-gray-300 border-t-indigo-600" />
        <span className="sr-only">Loading</span>
      </div>
    );
  }

  if (error || !experiment) {
    return <RetryableError message={error || 'Experiment not found'} onRetry={fetchData} context="experiment" />;
  }

  return (
    <div>
      <Breadcrumb items={[
        { label: 'Experiments', href: '/' },
        { label: experiment.name },
      ]} />

      {/* Header */}
      <div className="mb-6 flex items-start justify-between">
        <div>
          <div className="flex items-center gap-3">
            <h1 className="text-2xl font-bold text-gray-900">{experiment.name}</h1>
            <button
              type="button"
              onClick={handleCopyId}
              className="group relative flex h-6 w-6 items-center justify-center rounded-md text-gray-400 hover:bg-gray-100 hover:text-gray-600 focus:outline-none focus:ring-2 focus:ring-indigo-500"
              aria-label="Copy experiment ID"
              title="Copy experiment ID"
            >
              <svg
                className="h-4 w-4"
                fill="none"
                viewBox="0 0 24 24"
                stroke="currentColor"
              >
                <path
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  strokeWidth={2}
                  d="M8 5H6a2 2 0 00-2 2v12a2 2 0 002 2h10a2 2 0 002-2v-1M8 5a2 2 0 002 2h2a2 2 0 002-2M8 5a2 2 0 012-2h2a2 2 0 012 2m0 0h2a2 2 0 012 2v3m2 4H10m0 0l3-3m-3 3l3 3"
                />
              </svg>
            </button>
            <StateBadge state={experiment.state} />
            <TypeBadge type={experiment.type} />
            {(experiment.state === 'RUNNING' || experiment.state === 'CONCLUDED') && (
              <AdaptiveNBadge experimentId={experiment.experimentId} />
            )}
            {experiment.type === 'META' && experiment.metaConfig && experiment.variants.length > 0 && (() => {
              const firstConfig = experiment.metaConfig.variantBanditConfigs[0];
              if (!firstConfig) return null;
              const variantProb = experiment.variants[0]?.trafficFraction ?? 0;
              const armProb = firstConfig.arms.length > 0 ? 1 / firstConfig.arms.length : 0;
              return <TwoLevelIPWBadge variantProbability={variantProb} armProbability={armProb} />;
            })()}
          </div>
          <p className="mt-1 text-sm text-gray-600">{experiment.description}</p>
        </div>
        <div className="flex gap-2">
          <StateActions state={experiment.state} onTransition={handleTransition} />
          {experiment.state === 'CONCLUDED' && (
            <Link
              href={`/experiments/${experiment.experimentId}/results`}
              className="rounded-md bg-indigo-600 px-3 py-2 text-sm font-medium text-white hover:bg-indigo-700"
            >
              View Results
            </Link>
          )}
          {(experiment.type === 'MAB' || experiment.type === 'CONTEXTUAL_BANDIT') && experiment.state !== 'DRAFT' && (
            <Link
              href={`/experiments/${experiment.experimentId}/bandit`}
              className="rounded-md bg-purple-600 px-3 py-2 text-sm font-medium text-white hover:bg-purple-700"
            >
              Bandit Dashboard
            </Link>
          )}
          <Link
            href={`/experiments/${experiment.experimentId}/sql`}
            className="rounded-md border border-gray-300 bg-white px-3 py-2 text-sm font-medium text-gray-700 hover:bg-gray-50"
          >
            View SQL
          </Link>
        </div>
      </div>

      {/* Transitional state indicators */}
      {experiment.state === 'STARTING' && (
        <div className="mb-6">
          <StartingChecklist />
        </div>
      )}
      {experiment.state === 'CONCLUDING' && (
        <div className="mb-6">
          <ConcludingProgress />
        </div>
      )}

      {/* Metadata grid */}
      <div className="mb-6 grid grid-cols-2 gap-4 rounded-lg border border-gray-200 bg-white p-4 sm:grid-cols-4">
        <div>
          <dt className="text-xs font-medium uppercase text-gray-500">Owner</dt>
          <dd className="mt-1 text-sm text-gray-900">{experiment.ownerEmail}</dd>
        </div>
        <div>
          <dt className="text-xs font-medium uppercase text-gray-500">Primary Metric</dt>
          <dd className="mt-1 text-sm text-gray-900">{experiment.primaryMetricId}</dd>
        </div>
        <div>
          <dt className="text-xs font-medium uppercase text-gray-500">Created</dt>
          <dd className="mt-1 text-sm text-gray-900">{formatDate(experiment.createdAt)}</dd>
        </div>
        <div>
          <dt className="text-xs font-medium uppercase text-gray-500">Started</dt>
          <dd className="mt-1 text-sm text-gray-900">
            {experiment.startedAt ? formatDate(experiment.startedAt) : '—'}
          </dd>
        </div>
      </div>

      {/* Variant section: editable form for DRAFT, read-only table otherwise */}
      <section className="mb-6">
        <h2 className="mb-3 text-lg font-semibold text-gray-900">Variants</h2>
        <div className="overflow-hidden rounded-lg border border-gray-200 bg-white">
          {experiment.state === 'DRAFT' && canEdit ? (
            <VariantForm
              variants={experiment.variants}
              experimentType={experiment.type}
              onSave={handleSaveVariants}
            />
          ) : (
            <VariantTable variants={experiment.variants} />
          )}
        </div>
      </section>

      {/* Meta experiment: variant-to-bandit config panel */}
      {experiment.type === 'META' && experiment.metaConfig && (
        <MetaExperimentConfig
          variants={experiment.variants}
          metaConfig={experiment.metaConfig}
        />
      )}

      {/* Layer Allocation */}
      {experiment.layerId && (
        <section className="mb-6">
          <h2 className="mb-3 text-lg font-semibold text-gray-900">Layer Allocation</h2>
          <div className="overflow-hidden rounded-lg border border-gray-200 bg-white p-4">
            <LayerAllocationChart
              layerId={experiment.layerId}
              currentExperimentId={experiment.experimentId}
            />
          </div>
        </section>
      )}

      {/* Guardrails */}
      {experiment.guardrailConfigs.length > 0 && (
        <section className="mb-6">
          <h2 className="mb-3 text-lg font-semibold text-gray-900">Guardrails</h2>
          <div className="overflow-hidden rounded-lg border border-gray-200 bg-white">
            <table className="min-w-full divide-y divide-gray-200">
              <thead className="bg-gray-50">
                <tr>
                  <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">
                    Metric
                  </th>
                  <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">
                    Threshold
                  </th>
                  <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">
                    Breaches Required
                  </th>
                  <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">
                    Action
                  </th>
                </tr>
              </thead>
              <tbody className="divide-y divide-gray-200 bg-white">
                {experiment.guardrailConfigs.map((g) => (
                  <tr key={g.metricId}>
                    <td className="whitespace-nowrap px-4 py-3 text-sm font-medium text-gray-900">
                      {g.metricId}
                    </td>
                    <td className="whitespace-nowrap px-4 py-3 text-sm text-gray-600">
                      {g.threshold}
                    </td>
                    <td className="whitespace-nowrap px-4 py-3 text-sm text-gray-600">
                      {g.consecutiveBreachesRequired}
                    </td>
                    <td className="whitespace-nowrap px-4 py-3 text-sm">
                      <span
                        className={`inline-flex items-center rounded px-2 py-0.5 text-xs font-medium ${
                          experiment.guardrailAction === 'AUTO_PAUSE'
                            ? 'bg-red-50 text-red-700'
                            : 'bg-yellow-50 text-yellow-700'
                        }`}
                      >
                        {experiment.guardrailAction === 'AUTO_PAUSE' ? 'Auto-Pause' : 'Alert Only'}
                      </span>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </section>
      )}
    </div>
  );
}
