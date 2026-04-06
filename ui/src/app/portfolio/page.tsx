'use client';

import { useEffect, useState, useCallback } from 'react';
import dynamic from 'next/dynamic';
import Link from 'next/link';
import { getPortfolioAllocation, getPortfolioMetrics, getParetoFrontier } from '@/lib/api';
import type { PortfolioAllocationResult, PortfolioMetricsResult, ParetoFrontierResult } from '@/lib/types';
import { ExperimentPortfolioTable } from '@/components/experiment-portfolio-table';
import { RetryableError } from '@/components/retryable-error';

// Code-split: chart bundles loaded only when page renders
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

const WinRateChart = dynamic(
  () =>
    import('@/components/charts/win-rate-chart').then(
      (m) => ({ default: m.WinRateChart }),
    ),
  {
    ssr: false,
    loading: () => <ChartSkeleton height={240} />,
  },
);

const LearningRateChart = dynamic(
  () =>
    import('@/components/charts/learning-rate-chart').then(
      (m) => ({ default: m.LearningRateChart }),
    ),
  {
    ssr: false,
    loading: () => <ChartSkeleton height={240} />,
  },
);

const AnnualizedImpactChart = dynamic(
  () =>
    import('@/components/charts/annualized-impact-chart').then(
      (m) => ({ default: m.AnnualizedImpactChart }),
    ),
  {
    ssr: false,
    loading: () => <ChartSkeleton height={200} />,
  },
);

const ParetoFrontierPlot = dynamic(
  () =>
    import('@/components/charts/pareto-frontier-plot').then(
      (m) => ({ default: m.ParetoFrontierPlot }),
    ),
  {
    ssr: false,
    loading: () => <ChartSkeleton height={320} />,
  },
);

function ChartSkeleton({ height = 72 }: { height?: number }) {
  return (
    <div className="rounded-lg border border-gray-200 bg-white p-4">
      <div className="mb-2 h-4 w-48 animate-pulse rounded bg-gray-200" />
      <div className="animate-pulse rounded bg-gray-100" style={{ height }} />
    </div>
  );
}

export default function PortfolioDashboard() {
  const [result, setResult] = useState<PortfolioAllocationResult | null>(null);
  const [metrics, setMetrics] = useState<PortfolioMetricsResult | null>(null);
  const [pareto, setPareto] = useState<ParetoFrontierResult | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const fetchData = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const [allocationData, metricsData, paretoData] = await Promise.all([
        getPortfolioAllocation(),
        getPortfolioMetrics().catch(() => null),
        getParetoFrontier().catch(() => null),
      ]);
      setResult(allocationData);
      setMetrics(metricsData);
      setPareto(paretoData);
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

        {/* Win Rate + Learning Rate side-by-side */}
        {metrics && (
          <div className="grid grid-cols-1 gap-6 lg:grid-cols-2">
            <section aria-labelledby="win-rate-heading">
              <h2 id="win-rate-heading" className="sr-only">Win Rate Chart</h2>
              <WinRateChart data={metrics.winRates} />
            </section>
            <section aria-labelledby="learning-rate-heading">
              <h2 id="learning-rate-heading" className="sr-only">Learning Rate Chart</h2>
              <LearningRateChart data={metrics.learningRates} />
            </section>
          </div>
        )}

        {/* Annualized Impact */}
        {metrics && metrics.annualizedImpacts.length > 0 && (
          <section aria-labelledby="annualized-impact-heading">
            <h2 id="annualized-impact-heading" className="sr-only">Annualized Impact Chart</h2>
            <AnnualizedImpactChart data={metrics.annualizedImpacts} />
          </section>
        )}

        {/* Pareto Frontier */}
        {pareto && pareto.points.length > 0 && (
          <section aria-labelledby="pareto-heading">
            <h2 id="pareto-heading" className="sr-only">Pareto Frontier Plot</h2>
            <ParetoFrontierPlot points={pareto.points} frontierIds={pareto.frontierIds} />
          </section>
        )}

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
