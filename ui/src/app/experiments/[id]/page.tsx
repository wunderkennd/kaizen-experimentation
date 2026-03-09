'use client';

import { useEffect, useState, useCallback } from 'react';
import { useParams } from 'next/navigation';
import Link from 'next/link';
import type { Experiment, Variant } from '@/lib/types';
import { getExperiment, updateExperiment, startExperiment, concludeExperiment, archiveExperiment } from '@/lib/api';
import { formatDate } from '@/lib/utils';
import { StateBadge } from '@/components/state-badge';
import { TypeBadge } from '@/components/type-badge';
import { VariantTable } from '@/components/variant-table';
import { VariantForm } from '@/components/variant-form';
import { StateActions } from '@/components/state-actions';
import { StartingChecklist } from '@/components/starting-checklist';
import { ConcludingProgress } from '@/components/concluding-progress';

export default function ExperimentDetailPage() {
  const params = useParams<{ id: string }>();
  const [experiment, setExperiment] = useState<Experiment | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!params.id) return;

    getExperiment(params.id)
      .then(setExperiment)
      .catch((err) => setError(err.message))
      .finally(() => setLoading(false));
  }, [params.id]);

  const handleSaveVariants = useCallback(async (variants: Variant[]) => {
    if (!experiment) return;
    const updated = await updateExperiment({ ...experiment, variants });
    setExperiment(updated);
  }, [experiment]);

  const handleTransition = useCallback(async (action: 'start' | 'conclude' | 'archive') => {
    if (!experiment) return;
    const id = experiment.experimentId;
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
  }, [experiment]);

  if (loading) {
    return (
      <div className="flex items-center justify-center py-12">
        <div className="h-8 w-8 animate-spin rounded-full border-4 border-gray-300 border-t-indigo-600" />
      </div>
    );
  }

  if (error || !experiment) {
    return (
      <div className="rounded-md bg-red-50 p-4">
        <p className="text-sm text-red-700">
          {error || 'Experiment not found'}
        </p>
      </div>
    );
  }

  return (
    <div>
      {/* Breadcrumb */}
      <nav className="mb-4 text-sm text-gray-500">
        <Link href="/" className="hover:text-indigo-600">
          Experiments
        </Link>
        <span className="mx-2">/</span>
        <span className="text-gray-900">{experiment.name}</span>
      </nav>

      {/* Header */}
      <div className="mb-6 flex items-start justify-between">
        <div>
          <div className="flex items-center gap-3">
            <h1 className="text-2xl font-bold text-gray-900">{experiment.name}</h1>
            <StateBadge state={experiment.state} />
            <TypeBadge type={experiment.type} />
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
          {experiment.state === 'DRAFT' ? (
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
