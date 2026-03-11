'use client';

import { memo } from 'react';
import type { SrmResult } from '@/lib/types';
import { formatPValue } from '@/lib/utils';

interface SrmBannerProps {
  srmResult: SrmResult;
}

function SrmBannerInner({ srmResult }: SrmBannerProps) {
  if (!srmResult.isMismatch) return null;

  const observed = Object.entries(srmResult.observedCounts);
  const expected = srmResult.expectedCounts;

  return (
    <div className="mb-6 rounded-lg border border-red-300 bg-red-50 p-4" role="alert">
      <div className="flex items-start gap-3">
        <span className="text-lg" aria-hidden="true">!</span>
        <div>
          <h3 className="font-semibold text-red-800">
            Sample Ratio Mismatch Detected
          </h3>
          <p className="mt-1 text-sm text-red-700">
            Chi-squared = {srmResult.chiSquared.toFixed(2)}, p-value = {formatPValue(srmResult.pValue)}.
            Results may be unreliable due to imbalanced traffic allocation.
          </p>
          <div className="mt-2 text-sm text-red-700">
            <table className="text-left">
              <thead>
                <tr>
                  <th className="pr-4 font-medium">Variant</th>
                  <th className="pr-4 font-medium">Observed</th>
                  <th className="font-medium">Expected</th>
                </tr>
              </thead>
              <tbody>
                {observed.map(([variantId, count]) => (
                  <tr key={variantId}>
                    <td className="pr-4">{variantId}</td>
                    <td className="pr-4">{count.toLocaleString()}</td>
                    <td>{(expected[variantId] ?? 0).toLocaleString()}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      </div>
    </div>
  );
}

export const SrmBanner = memo(SrmBannerInner);
