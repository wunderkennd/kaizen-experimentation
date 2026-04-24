'use client';

import { useState } from 'react';
import Link from 'next/link';
import type { PortfolioExperiment } from '@/lib/types';
import { ConflictBadge } from './conflict-badge';

type SortKey = 'name' | 'effectSize' | 'variance' | 'allocatedTrafficPct' | 'priorityScore';
type SortDir = 'asc' | 'desc';

interface ExperimentPortfolioTableProps {
  experiments: PortfolioExperiment[];
}

function sortExperiments(
  experiments: PortfolioExperiment[],
  key: SortKey,
  dir: SortDir,
): PortfolioExperiment[] {
  return [...experiments].sort((a, b) => {
    const av = a[key];
    const bv = b[key];
    if (typeof av === 'string' && typeof bv === 'string') {
      return dir === 'asc' ? av.localeCompare(bv) : bv.localeCompare(av);
    }
    const an = av as number;
    const bn = bv as number;
    return dir === 'asc' ? an - bn : bn - an;
  });
}

export function ExperimentPortfolioTable({ experiments }: ExperimentPortfolioTableProps) {
  const [sortKey, setSortKey] = useState<SortKey>('priorityScore');
  const [sortDir, setSortDir] = useState<SortDir>('desc');

  function handleSort(key: SortKey) {
    if (key === sortKey) {
      setSortDir((d) => (d === 'asc' ? 'desc' : 'asc'));
    } else {
      setSortKey(key);
      setSortDir('desc');
    }
  }

  const sorted = sortExperiments(experiments, sortKey, sortDir);

  function SortIndicator({ col }: { col: SortKey }) {
    if (col !== sortKey) return <span className="ml-1 text-gray-300 transition-colors group-hover:text-gray-400" aria-hidden="true">↕</span>;
    return (
      <span className="ml-1 text-indigo-600 font-bold" aria-hidden="true">
        {sortDir === 'asc' ? '↑' : '↓'}
      </span>
    );
  }

  function thProps(col: SortKey, align: 'left' | 'right' = 'left') {
    const isActive = col === sortKey;
    return {
      scope: 'col' as const,
      className: `py-3 text-xs font-medium uppercase tracking-wide text-gray-500 ${align === 'right' ? 'text-right pr-4' : 'pl-4 pr-3'}`,
      'aria-sort': (isActive
        ? sortDir === 'asc' ? 'ascending' : 'descending'
        : 'none') as React.AriaAttributes['aria-sort'],
    };
  }

  if (experiments.length === 0) {
    return (
      <div className="rounded-lg border border-gray-200 bg-white py-12 text-center">
        <p className="text-sm text-gray-500">No active experiments in portfolio.</p>
      </div>
    );
  }

  return (
    <div className="overflow-x-auto rounded-lg border border-gray-200 bg-white" data-testid="portfolio-table">
      <table className="min-w-full divide-y divide-gray-200">
        <thead className="bg-gray-50">
          <tr>
            <th {...thProps('name')}>
              <button
                type="button"
                onClick={() => handleSort('name')}
                className="group inline-flex items-center gap-1 rounded-sm focus:outline-none focus:ring-2 focus:ring-indigo-500 focus:ring-offset-1 hover:text-gray-700"
              >
                Experiment <SortIndicator col="name" />
              </button>
            </th>
            <th {...thProps('effectSize', 'right')}>
              <button
                type="button"
                onClick={() => handleSort('effectSize')}
                className="group ml-auto inline-flex items-center gap-1 rounded-sm focus:outline-none focus:ring-2 focus:ring-indigo-500 focus:ring-offset-1 hover:text-gray-700"
              >
                Effect Size <SortIndicator col="effectSize" />
              </button>
            </th>
            <th {...thProps('variance', 'right')}>
              <button
                type="button"
                onClick={() => handleSort('variance')}
                className="group ml-auto inline-flex items-center gap-1 rounded-sm focus:outline-none focus:ring-2 focus:ring-indigo-500 focus:ring-offset-1 hover:text-gray-700"
              >
                Variance <SortIndicator col="variance" />
              </button>
            </th>
            <th {...thProps('allocatedTrafficPct', 'right')}>
              <button
                type="button"
                onClick={() => handleSort('allocatedTrafficPct')}
                className="group ml-auto inline-flex items-center gap-1 rounded-sm focus:outline-none focus:ring-2 focus:ring-indigo-500 focus:ring-offset-1 hover:text-gray-700"
              >
                Traffic % <SortIndicator col="allocatedTrafficPct" />
              </button>
            </th>
            <th {...thProps('priorityScore', 'right')}>
              <button
                type="button"
                onClick={() => handleSort('priorityScore')}
                className="group ml-auto inline-flex items-center gap-1 rounded-sm focus:outline-none focus:ring-2 focus:ring-indigo-500 focus:ring-offset-1 hover:text-gray-700"
              >
                Priority Score <SortIndicator col="priorityScore" />
              </button>
            </th>
            <th scope="col" className="py-3 pr-4 text-right text-xs font-medium uppercase tracking-wide text-gray-500">
              Conflicts
            </th>
          </tr>
        </thead>
        <tbody className="divide-y divide-gray-100 bg-white">
          {sorted.map((exp) => (
            <tr key={exp.experimentId} className="hover:bg-gray-50">
              <td className="py-3 pl-4 pr-3">
                <Link
                  href={`/experiments/${exp.experimentId}`}
                  className="font-medium text-indigo-600 hover:text-indigo-800"
                  data-testid={`portfolio-row-name-${exp.experimentId}`}
                >
                  {exp.name}
                </Link>
              </td>
              <td className="py-3 pr-4 text-right font-mono text-sm text-gray-700" data-testid="col-effect-size">
                {exp.effectSize >= 0 ? '+' : ''}{exp.effectSize.toFixed(4)}
              </td>
              <td className="py-3 pr-4 text-right font-mono text-sm text-gray-700" data-testid="col-variance">
                {exp.variance.toFixed(6)}
              </td>
              <td className="py-3 pr-4 text-right font-mono text-sm text-gray-700" data-testid="col-traffic">
                {(exp.allocatedTrafficPct * 100).toFixed(1)}%
              </td>
              <td className="py-3 pr-4 text-right font-mono text-sm text-gray-700" data-testid="col-priority">
                {exp.priorityScore.toFixed(3)}
              </td>
              <td className="py-3 pr-4 text-right">
                <ConflictBadge experiment={exp} allExperiments={experiments} />
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
