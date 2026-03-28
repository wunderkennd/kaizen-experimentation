'use client';

import { memo } from 'react';
import type { ConstraintStatus } from '@/lib/types';

interface ConstraintStatusTableProps {
  constraints: ConstraintStatus[];
}

function ConstraintStatusTableInner({ constraints }: ConstraintStatusTableProps) {
  if (constraints.length === 0) {
    return (
      <p className="py-4 text-center text-sm text-gray-500">No LP constraints configured.</p>
    );
  }

  return (
    <div className="overflow-hidden rounded-lg border border-gray-200 bg-white">
      <table className="min-w-full divide-y divide-gray-200">
        <thead className="bg-gray-50">
          <tr>
            <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">
              Constraint
            </th>
            <th className="px-4 py-3 text-right text-xs font-medium uppercase tracking-wider text-gray-500">
              Current Value
            </th>
            <th className="px-4 py-3 text-right text-xs font-medium uppercase tracking-wider text-gray-500">
              Limit
            </th>
            <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">
              Status
            </th>
          </tr>
        </thead>
        <tbody className="divide-y divide-gray-200">
          {constraints.map((c) => (
            <tr
              key={c.label}
              className={c.isSatisfied ? '' : 'bg-red-50'}
            >
              <td className="whitespace-nowrap px-4 py-3 text-sm font-medium text-gray-900">
                {c.label}
              </td>
              <td className={`whitespace-nowrap px-4 py-3 text-right text-sm ${c.isSatisfied ? 'text-gray-600' : 'font-semibold text-red-700'}`}>
                {c.currentValue.toFixed(4)}
              </td>
              <td className="whitespace-nowrap px-4 py-3 text-right text-sm text-gray-600">
                {c.limit.toFixed(4)}
              </td>
              <td className="whitespace-nowrap px-4 py-3 text-sm">
                {c.isSatisfied ? (
                  <span className="inline-flex items-center rounded-full bg-green-100 px-2.5 py-0.5 text-xs font-medium text-green-800">
                    SATISFIED
                  </span>
                ) : (
                  <span className="inline-flex items-center rounded-full bg-red-100 px-2.5 py-0.5 text-xs font-medium text-red-800">
                    VIOLATED
                  </span>
                )}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

export const ConstraintStatusTable = memo(ConstraintStatusTableInner);
