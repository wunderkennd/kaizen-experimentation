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
      <table className="min-w-full divide-y divide-gray-200" aria-label="LP constraint status summary">
        <thead className="bg-gray-50">
          <tr>
            <th scope="col" className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">
              Constraint
            </th>
            <th scope="col" className="px-4 py-3 text-right text-xs font-medium uppercase tracking-wider text-gray-500">
              Current Value
            </th>
            <th scope="col" className="px-4 py-3 text-right text-xs font-medium uppercase tracking-wider text-gray-500">
              Limit
            </th>
            <th scope="col" className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">
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
                  <span className="inline-flex items-center gap-1 rounded-full bg-green-100 px-2.5 py-0.5 text-xs font-medium text-green-800">
                    <svg className="h-3 w-3 text-green-600" aria-hidden="true" fill="currentColor" viewBox="0 0 20 20">
                      <path fillRule="evenodd" d="M16.707 5.293a1 1 0 010 1.414l-8 8a1 1 0 01-1.414 0l-4-4a1 1 0 011.414-1.414L8 12.586l7.293-7.293a1 1 0 011.414 0z" clipRule="evenodd" />
                    </svg>
                    SATISFIED
                  </span>
                ) : (
                  <span className="inline-flex items-center gap-1 rounded-full bg-red-100 px-2.5 py-0.5 text-xs font-medium text-red-800">
                    <svg className="h-3 w-3 text-red-600" aria-hidden="true" fill="currentColor" viewBox="0 0 20 20">
                      <path fillRule="evenodd" d="M4.293 4.293a1 1 0 011.414 0L10 8.586l4.293-4.293a1 1 0 111.414 1.414L11.414 10l4.293 4.293a1 1 0 01-1.414 1.414L10 11.414l-4.293 4.293a1 1 0 01-1.414-1.414L8.586 10 4.293 5.707a1 1 0 010-1.414z" clipRule="evenodd" />
                    </svg>
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
