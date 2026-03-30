'use client';

import { memo } from 'react';
import type { Variant, MetaConfig } from '@/lib/types';

const BANDIT_LABELS: Record<string, string> = {
  THOMPSON_SAMPLING: 'Thompson Sampling',
  LINEAR_UCB: 'Linear UCB',
  THOMPSON_LINEAR: 'Thompson Linear',
  NEURAL_CONTEXTUAL: 'Neural Contextual',
};

interface MetaExperimentConfigProps {
  variants: Variant[];
  metaConfig: MetaConfig;
}

function MetaExperimentConfigInner({ variants, metaConfig }: MetaExperimentConfigProps) {
  const configMap = new Map(metaConfig.variantBanditConfigs.map((c) => [c.variantId, c]));

  return (
    <section className="mb-6">
      <h2 className="mb-3 text-lg font-semibold text-gray-900">Meta Experiment — Variant Bandit Mapping</h2>
      <p className="mb-3 text-xs text-gray-500">
        Each top-level variant runs an independent bandit. Assignment is two-level: P(variant) × P(arm|variant).
      </p>
      <div className="overflow-hidden rounded-lg border border-gray-200 bg-white">
        <table className="min-w-full divide-y divide-gray-200">
          <thead className="bg-gray-50">
            <tr>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">
                Variant
              </th>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">
                Variant ID
              </th>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">
                Bandit Type
              </th>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">
                Arms
              </th>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">
                Traffic
              </th>
            </tr>
          </thead>
          <tbody className="divide-y divide-gray-200 bg-white">
            {variants.map((v) => {
              const config = configMap.get(v.variantId);
              return (
                <tr key={v.variantId}>
                  <td className="whitespace-nowrap px-4 py-3 text-sm font-medium text-gray-900">
                    {v.name}
                    {v.isControl && (
                      <span className="ml-2 inline-flex items-center rounded-full bg-gray-100 px-2 py-0.5 text-xs font-medium text-gray-600">
                        control
                      </span>
                    )}
                  </td>
                  <td className="whitespace-nowrap px-4 py-3 font-mono text-xs text-gray-500">
                    {v.variantId}
                  </td>
                  <td className="whitespace-nowrap px-4 py-3 text-sm text-gray-700">
                    {config ? (
                      <span className="inline-flex items-center rounded-full bg-purple-100 px-2 py-0.5 text-xs font-medium text-purple-800">
                        {BANDIT_LABELS[config.banditType] ?? config.banditType}
                      </span>
                    ) : (
                      <span className="text-gray-400">—</span>
                    )}
                  </td>
                  <td className="px-4 py-3 text-sm text-gray-700">
                    {config && config.arms.length > 0 ? (
                      <div className="flex flex-wrap gap-1">
                        {config.arms.map((arm) => (
                          <span
                            key={arm}
                            className="inline-flex items-center rounded bg-indigo-50 px-1.5 py-0.5 text-xs font-medium text-indigo-700"
                          >
                            {arm}
                          </span>
                        ))}
                      </div>
                    ) : (
                      <span className="text-gray-400">—</span>
                    )}
                  </td>
                  <td className="whitespace-nowrap px-4 py-3 text-sm text-gray-600">
                    {(v.trafficFraction * 100).toFixed(1)}%
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    </section>
  );
}

export const MetaExperimentConfig = memo(MetaExperimentConfigInner);
