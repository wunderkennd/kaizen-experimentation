'use client';

import type { Variant } from '@/lib/types';
import { formatPercent, truncateJson } from '@/lib/utils';
import { CopyButton } from './copy-button';

interface VariantTableProps {
  variants: Variant[];
}

export function VariantTable({ variants }: VariantTableProps) {
  return (
    <div className="overflow-x-auto">
      <table className="min-w-full divide-y divide-gray-200">
        <thead className="bg-gray-50">
          <tr>
            <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">
              Name
            </th>
            <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">
              ID
            </th>
            <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">
              Traffic
            </th>
            <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">
              Role
            </th>
            <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">
              Payload
            </th>
          </tr>
        </thead>
        <tbody className="divide-y divide-gray-200 bg-white">
          {variants.map((v) => (
            <tr key={v.variantId}>
              <td className="whitespace-nowrap px-4 py-3 text-sm font-medium text-gray-900">
                {v.name}
              </td>
              <td className="whitespace-nowrap px-4 py-3 text-sm text-gray-600">
                <div className="flex items-center gap-2">
                  <code className="rounded bg-gray-100 px-1.5 py-0.5 text-xs text-gray-500">
                    {v.variantId}
                  </code>
                  <CopyButton
                    value={v.variantId}
                    label="Copy variant ID"
                    successMessage="Variant ID copied"
                  />
                </div>
              </td>
              <td className="whitespace-nowrap px-4 py-3 text-sm text-gray-600">
                {formatPercent(v.trafficFraction)}
              </td>
              <td className="whitespace-nowrap px-4 py-3 text-sm">
                {v.isControl ? (
                  <span className="inline-flex items-center rounded bg-emerald-50 px-2 py-0.5 text-xs font-medium text-emerald-700 ring-1 ring-inset ring-emerald-600/20">
                    Control
                  </span>
                ) : (
                  <span className="text-gray-500">Treatment</span>
                )}
              </td>
              <td className="px-4 py-3 text-sm">
                <code className="rounded bg-gray-100 px-1.5 py-0.5 text-xs text-gray-700">
                  {truncateJson(v.payloadJson)}
                </code>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
