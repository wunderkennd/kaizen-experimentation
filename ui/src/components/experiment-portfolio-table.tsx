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
    const isActive = col === sortKey;
    if (!isActive) {
      return (
        <svg className="ml-1 h-3 w-3 text-gray-300 transition-colors group-hover:text-gray-400" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2} aria-hidden="true">
          <path strokeLinecap="round" strokeLinejoin="round" d="M7 16V4m0 0L3 8m4-4l4 4m6 0v12m0 0l4-4m-4 4l-4-4" />
        </svg>
      );
    }
    return sortDir === 'asc' ? (
      <svg className="ml-1 h-3 w-3 text-indigo-600" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={3} aria-hidden="true">
        <path strokeLinecap="round" strokeLinejoin="round" d="M5 15l7-7 7 7" />
      </svg>
    ) : (
      <svg className="ml-1 h-3 w-3 text-indigo-600" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={3} aria-hidden="true">
        <path strokeLinecap="round" strokeLinejoin="round" d="M19 9l-7 7-7-7" />
      </svg>
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

  const LABEL_MAP: Record<SortKey, string> = {
    name: 'Experiment',
    effectSize: 'Effect Size',
    variance: 'Variance',
    allocatedTrafficPct: 'Traffic %',
    priorityScore: 'Priority Score',
  };

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
                className="group inline-flex items-center gap-1 rounded-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500 focus-visible:ring-offset-2 hover:text-gray-700"
                title={`Sort by ${LABEL_MAP.name}`}
              >
                {LABEL_MAP.name} <SortIndicator col="name" />
              </button>
            </th>
            <th {...thProps('effectSize', 'right')}>
              <button
                type="button"
                onClick={() => handleSort('effectSize')}
                className="group ml-auto inline-flex items-center gap-1 rounded-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500 focus-visible:ring-offset-2 hover:text-gray-700"
                title={`Sort by ${LABEL_MAP.effectSize}`}
              >
                {LABEL_MAP.effectSize} <SortIndicator col="effectSize" />
              </button>
            </th>
            <th {...thProps('variance', 'right')}>
              <button
                type="button"
                onClick={() => handleSort('variance')}
                className="group ml-auto inline-flex items-center gap-1 rounded-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500 focus-visible:ring-offset-2 hover:text-gray-700"
                title={`Sort by ${LABEL_MAP.variance}`}
              >
                {LABEL_MAP.variance} <SortIndicator col="variance" />
              </button>
            </th>
            <th {...thProps('allocatedTrafficPct', 'right')}>
              <button
                type="button"
                onClick={() => handleSort('allocatedTrafficPct')}
                className="group ml-auto inline-flex items-center gap-1 rounded-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500 focus-visible:ring-offset-2 hover:text-gray-700"
                title={`Sort by ${LABEL_MAP.allocatedTrafficPct}`}
              >
                {LABEL_MAP.allocatedTrafficPct} <SortIndicator col="allocatedTrafficPct" />
              </button>
            </th>
            <th {...thProps('priorityScore', 'right')}>
              <button
                type="button"
                onClick={() => handleSort('priorityScore')}
                className="group ml-auto inline-flex items-center gap-1 rounded-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500 focus-visible:ring-offset-2 hover:text-gray-700"
                title={`Sort by ${LABEL_MAP.priorityScore}`}
              >
                {LABEL_MAP.priorityScore} <SortIndicator col="priorityScore" />
              </button>
            </th>
            <th scope="col" className="py-3 pr-4 text-right text-xs font-medium uppercase tracking-wide text-gray-500">
              Conflicts
            </th>
          </tr>
        </thead>
        <tbody className="divide-y divide-gray-100 bg-white">
          {sorted.map((exp) => (
            <tr
              key={exp.experimentId}
              className="hover:bg-gray-50 focus-within:bg-gray-50 focus-within:outline-none focus-within:ring-2 focus-within:ring-inset focus-within:ring-indigo-500"
            >
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
