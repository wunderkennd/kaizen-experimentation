'use client';

import { useEffect, useState, useCallback } from 'react';
import dynamic from 'next/dynamic';
import Link from 'next/link';
import { getPortfolioAllocation } from '@/lib/api';
import type { PortfolioAllocationResult } from '@/lib/types';
import { ExperimentPortfolioTable } from '@/components/experiment-portfolio-table';
import { RetryableError } from '@/components/retryable-error';

// Code-split: chart bundle loaded only when page renders
const BudgetAllocationChart = dynamic(
  () =>
    import('@/components/charts/budget-allocation-chart').then(
      (m) => ({ default: m.BudgetAllocationChart }),
    ),
  {
    ssr: false,
    loading: () => <ChartSkeleton />,
  },
);

function ChartSkeleton() {
  return (
    <div className="rounded-lg border border-gray-200 bg-white p-4">
      <div className="mb-2 h-4 w-48 animate-pulse rounded bg-gray-200" />
      <div className="h-[72px] animate-pulse rounded bg-gray-100" />
    </div>
  );
}

export default function PortfolioDashboard() {
  const [result, setResult] = useState<PortfolioAllocationResult | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const fetchData = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const data = await getPortfolioAllocation();
      setResult(data);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load portfolio allocation data.');
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    fetchData();
  }, [fetchData]);

  if (loading && !result) {
    return (
      <div className="flex items-center justify-center py-12" role="status" aria-label="Loading">
        <div className="h-8 w-8 animate-spin rounded-full border-4 border-gray-300 border-t-indigo-600" />
        <span className="sr-only">Loading</span>
      </div>
    );
  }

  if (error && !result) {
    return <RetryableError message={error} onRetry={fetchData} context="portfolio allocation data" />;
  }

  const experiments = result?.experiments ?? [];

  return (
    <div>
      {/* Breadcrumb */}
      <nav aria-label="Breadcrumb" className="mb-4 flex items-center gap-2 text-sm text-gray-500">
        <span className="text-gray-900 font-medium">Portfolio</span>
      </nav>

      <div className="mb-6 flex flex-col gap-4 sm:flex-row sm:items-center sm:justify-between">
        <div>
          <h1 className="text-2xl font-bold text-gray-900">Portfolio Dashboard</h1>
          <p className="mt-1 text-sm text-gray-500">
            Traffic budget allocation and priority scores across active experiments.
          </p>
        </div>
        <Link
          href="/portfolio/provider-health"
          className="text-sm font-medium text-indigo-600 hover:text-indigo-800"
          data-testid="provider-health-link"
        >
          Provider Health →
        </Link>
      </div>

      {loading && result && (
        <div className="mb-4 flex items-center gap-2 text-sm text-gray-500" role="status">
          <div className="h-4 w-4 animate-spin rounded-full border-2 border-gray-300 border-t-indigo-600" />
          Refreshing…
        </div>
      )}

      {error && result && (
        <div className="mb-4 rounded-md border border-yellow-200 bg-yellow-50 px-4 py-2 text-sm text-yellow-800" role="alert">
          {error}
        </div>
      )}

      <div className="flex flex-col gap-6">
        <section aria-labelledby="budget-chart-heading">
          <h2 id="budget-chart-heading" className="sr-only">Budget Allocation Chart</h2>
          <BudgetAllocationChart experiments={experiments} />
        </section>

        <section aria-labelledby="portfolio-table-heading">
          <h2 id="portfolio-table-heading" className="mb-3 text-base font-semibold text-gray-900">
            Active Experiments
          </h2>
          <ExperimentPortfolioTable experiments={experiments} />
        </section>
      </div>

      {result && (
        <p className="mt-4 text-xs text-gray-400" data-testid="computed-at">
          Data computed at: {new Date(result.computedAt).toLocaleString()}
        </p>
      )}
    </div>
  );
}
