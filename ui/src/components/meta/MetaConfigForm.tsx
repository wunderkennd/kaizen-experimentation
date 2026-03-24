'use client';

import { useWizard } from '../wizard/wizard-context';
import type { BanditAlgorithm, MetaConfig, VariantBanditConfig } from '@/lib/types';

const ALGORITHMS: { value: BanditAlgorithm; label: string }[] = [
  { value: 'THOMPSON_SAMPLING', label: 'Thompson Sampling' },
  { value: 'LINEAR_UCB', label: 'Linear UCB' },
  { value: 'THOMPSON_LINEAR', label: 'Thompson Linear' },
  { value: 'NEURAL_CONTEXTUAL', label: 'Neural Contextual' },
];

export function MetaConfigForm() {
  const { state, dispatch } = useWizard();
  const { variants, metaConfig } = state;

  const configMap = new Map<string, VariantBanditConfig>(
    metaConfig.variantBanditConfigs.map((c) => [c.variantId, c]),
  );

  const updateVariantConfig = (
    variantId: string,
    patch: Partial<Pick<VariantBanditConfig, 'banditType' | 'arms'>>,
  ) => {
    const existing = configMap.get(variantId) ?? {
      variantId,
      banditType: 'THOMPSON_SAMPLING' as BanditAlgorithm,
      arms: [],
    };
    const updated: VariantBanditConfig = { ...existing, ...patch };
    const others = metaConfig.variantBanditConfigs.filter((c) => c.variantId !== variantId);
    const next: MetaConfig = { variantBanditConfigs: [...others, updated] };
    dispatch({ type: 'SET_FIELD', field: 'metaConfig', value: next });
  };

  if (variants.length === 0) {
    return (
      <p className="rounded-md bg-gray-50 p-4 text-sm text-gray-600">
        No variants configured. Add variants in the Variants step first.
      </p>
    );
  }

  return (
    <div className="space-y-6">
      <p className="text-sm text-gray-600">
        Configure an independent bandit policy for each top-level variant. The compound assignment
        probability P(variant) × P(arm|variant) is used for IPW-corrected analysis.
      </p>
      {variants.map((v) => {
        const config = configMap.get(v.variantId) ?? {
          variantId: v.variantId,
          banditType: 'THOMPSON_SAMPLING' as BanditAlgorithm,
          arms: [],
        };
        const armsStr = config.arms.join(', ');

        return (
          <fieldset
            key={v.variantId}
            className="rounded-lg border border-gray-200 p-4"
          >
            <legend className="px-1 text-sm font-semibold text-gray-900">
              {v.name}
              {v.isControl && (
                <span className="ml-2 inline-flex items-center rounded-full bg-gray-100 px-2 py-0.5 text-xs font-medium text-gray-600">
                  control
                </span>
              )}
            </legend>
            <div className="mt-3 grid grid-cols-1 gap-4 sm:grid-cols-2">
              <div>
                <label
                  htmlFor={`bandit-type-${v.variantId}`}
                  className="block text-sm font-medium text-gray-700"
                >
                  Bandit Algorithm <span className="text-red-500">*</span>
                </label>
                <select
                  id={`bandit-type-${v.variantId}`}
                  value={config.banditType}
                  onChange={(e) =>
                    updateVariantConfig(v.variantId, { banditType: e.target.value as BanditAlgorithm })
                  }
                  className="mt-1 block w-full rounded-md border border-gray-300 px-3 py-2 text-sm shadow-sm focus:border-indigo-500 focus:ring-indigo-500"
                >
                  {ALGORITHMS.map((a) => (
                    <option key={a.value} value={a.value}>
                      {a.label}
                    </option>
                  ))}
                </select>
              </div>
              <div>
                <label
                  htmlFor={`arms-${v.variantId}`}
                  className="block text-sm font-medium text-gray-700"
                >
                  Arms <span className="text-red-500">*</span>
                </label>
                <input
                  id={`arms-${v.variantId}`}
                  type="text"
                  value={armsStr}
                  onChange={(e) =>
                    updateVariantConfig(v.variantId, {
                      arms: e.target.value.split(',').map((s) => s.trim()).filter(Boolean),
                    })
                  }
                  placeholder="arm-a, arm-b, arm-c"
                  aria-describedby={`arms-help-${v.variantId}`}
                  className="mt-1 block w-full rounded-md border border-gray-300 px-3 py-2 text-sm shadow-sm focus:border-indigo-500 focus:ring-indigo-500"
                />
                <p id={`arms-help-${v.variantId}`} className="mt-1 text-xs text-gray-500">
                  Comma-separated arm identifiers (e.g. arm-control, arm-boost-01)
                </p>
              </div>
            </div>
          </fieldset>
        );
      })}
    </div>
  );
}
