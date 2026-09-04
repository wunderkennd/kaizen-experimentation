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
        <div className="flex-shrink-0 pt-0.5">
          <svg
            className="h-5 w-5 text-red-600"
            viewBox="0 0 20 20"
            fill="currentColor"
            aria-hidden="true"
          >
            <path
              fillRule="evenodd"
              d="M8.485 2.495c.673-1.167 2.357-1.167 3.03 0l6.28 10.875c.673 1.167-.17 2.625-1.516 2.625H3.72c-1.347 0-2.189-1.458-1.515-2.625L8.485 2.495zM10 5a.75.75 0 01.75.75v3.5a.75.75 0 01-1.5 0v-3.5A.75.75 0 0110 5zm0 9a1 1 0 100-2 1 1 0 000 2z"
              clipRule="evenodd"
            />
          </svg>
        </div>
        <div>
          <div className="flex items-center gap-2">
            <h3 className="font-semibold text-red-800">
              Sample Ratio Mismatch Detected
            </h3>
            <span className="inline-flex items-center rounded-full bg-red-100 px-2 py-0.5 text-xs font-semibold text-red-800">
              Mismatch Detected
            </span>
          </div>
          <p className="mt-1 text-sm text-red-700">
            Chi-squared = {srmResult.chiSquared.toFixed(2)}, p-value = {formatPValue(srmResult.pValue)}.
            Results may be unreliable due to imbalanced traffic allocation.
          </p>
          <div className="mt-2 text-sm text-red-700">
            <table className="text-left" aria-label="Sample ratio mismatch observed and expected counts">
              <thead>
                <tr>
                  <th scope="col" className="pr-4 font-medium">Variant</th>
                  <th scope="col" className="pr-4 font-medium">Observed</th>
                  <th scope="col" className="font-medium">Expected</th>
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
