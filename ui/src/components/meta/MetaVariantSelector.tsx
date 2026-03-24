'use client';

import { memo } from 'react';
import type { Variant, MetaConfig } from '@/lib/types';

const BANDIT_SHORT: Record<string, string> = {
  THOMPSON_SAMPLING: 'TS',
  LINEAR_UCB: 'LinUCB',
  THOMPSON_LINEAR: 'TL',
  NEURAL_CONTEXTUAL: 'Neural',
};

interface MetaVariantSelectorProps {
  variants: Variant[];
  metaConfig: MetaConfig;
  selectedVariantId: string;
  onChange: (variantId: string) => void;
  id?: string;
  label?: string;
}

function MetaVariantSelectorInner({
  variants,
  metaConfig,
  selectedVariantId,
  onChange,
  id = 'meta-variant-selector',
  label = 'Select Variant',
}: MetaVariantSelectorProps) {
  const configMap = new Map(metaConfig.variantBanditConfigs.map((c) => [c.variantId, c]));

  return (
    <div>
      {label && (
        <label htmlFor={id} className="block text-sm font-medium text-gray-700">
          {label}
        </label>
      )}
      <select
        id={id}
        value={selectedVariantId}
        onChange={(e) => onChange(e.target.value)}
        className="mt-1 block w-full rounded-md border border-gray-300 px-3 py-2 text-sm shadow-sm focus:border-indigo-500 focus:ring-indigo-500"
        aria-label={label}
      >
        <option value="">Select a variant…</option>
        {variants.map((v) => {
          const config = configMap.get(v.variantId);
          const policy = config ? ` [${BANDIT_SHORT[config.banditType] ?? config.banditType}, ${config.arms.length} arms]` : ' [no policy]';
          return (
            <option key={v.variantId} value={v.variantId}>
              {v.name}{policy}
            </option>
          );
        })}
      </select>
    </div>
  );
}

export const MetaVariantSelector = memo(MetaVariantSelectorInner);
