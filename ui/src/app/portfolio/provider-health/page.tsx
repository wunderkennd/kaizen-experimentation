'use client';

import { useEffect, useState, useCallback } from 'react';
import dynamic from 'next/dynamic';
import Link from 'next/link';
import { getProviderHealth } from '@/lib/api';
import type { ProviderHealthResult, ProviderHealthSeries } from '@/lib/types';
import { RetryableError } from '@/components/retryable-error';

// Code-split: chart bundle loaded only when page is rendered
const CatalogCoverageChart = dynamic(
  () =>
    import('@/components/charts/provider-health-charts').then(
      (m) => ({ default: m.CatalogCoverageChart }),
    ),
  {
    ssr: false,
    loading: () => <ChartSkeleton />,
  },
);

const ProviderGiniChart = dynamic(
  () =>
    import('@/components/charts/provider-health-charts').then(
      (m) => ({ default: m.ProviderGiniChart }),
    ),
  {
    ssr: false,
    loading: () => <ChartSkeleton />,
  },
);

const LongTailImpressionChart = dynamic(
  () =>
    import('@/components/charts/provider-health-charts').then(
      (m) => ({ default: m.LongTailImpressionChart }),
    ),
  {
    ssr: false,
    loading: () => <ChartSkeleton />,
  },
);

function ChartSkeleton() {
  return (
    <div className="rounded-lg border border-gray-200 bg-white p-4">
      <div className="mb-2 h-4 w-40 animate-pulse rounded bg-gray-200" />
      <div className="h-[260px] animate-pulse rounded bg-gray-100" />
    </div>
  );
}

export default function ProviderHealthPage() {
  const [result, setResult] = useState<ProviderHealthResult | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [selectedProvider, setSelectedProvider] = useState<string>('');

  const fetchData = useCallback(async (providerId?: string) => {
    setLoading(true);
    setError(null);
    try {
      const data = await getProviderHealth(providerId || undefined);
      setResult(data);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load provider health data.');
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    fetchData();
  }, [fetchData]);

  function handleProviderChange(e: React.ChangeEvent<HTMLSelectElement>) {
    const value = e.target.value;
    setSelectedProvider(value);
    fetchData(value || undefined);
  }

  const series: ProviderHealthSeries[] = result?.series ?? [];
  const providers = result?.providers ?? [];

  if (loading && !result) {
    return (
      <div className="flex items-center justify-center py-12" role="status" aria-label="Loading">
        <div className="h-8 w-8 animate-spin rounded-full border-4 border-gray-300 border-t-indigo-600" />
        <span className="sr-only">Loading</span>
      </div>
    );
  }

  if (error && !result) {
    return <RetryableError message={error} onRetry={() => fetchData(selectedProvider || undefined)} context="provider health data" />;
  }

  return (
    <div>
      {/* Breadcrumb */}
      <nav aria-label="Breadcrumb" className="mb-4 flex items-center gap-2 text-sm text-gray-500">
        <Link href="/portfolio" className="hover:text-gray-700">Portfolio</Link>
        <span aria-hidden="true">/</span>
        <span className="text-gray-900 font-medium">Provider Health</span>
      </nav>

      <div className="mb-6 flex flex-col gap-4 sm:flex-row sm:items-center sm:justify-between">
        <div>
          <h1 className="text-2xl font-bold text-gray-900">Provider Health</h1>
          <p className="mt-1 text-sm text-gray-500">
            Catalog coverage, Gini concentration, and long-tail impression share across running experiments.
          </p>
        </div>

        {/* Provider filter */}
        <div className="flex items-center gap-2">
          <label htmlFor="provider-select" className="text-sm font-medium text-gray-700">
            Provider
          </label>
          <select
            id="provider-select"
            value={selectedProvider}
            onChange={handleProviderChange}
            className="rounded-md border border-gray-300 bg-white px-3 py-1.5 text-sm shadow-sm focus:border-indigo-500 focus:outline-none focus:ring-1 focus:ring-indigo-500"
            data-testid="provider-filter"
            aria-label="Filter by provider"
          >
            <option value="">All providers</option>
            {providers.map((p) => (
              <option key={p.providerId} value={p.providerId}>
                {p.providerName}
              </option>
            ))}
          </select>
        </div>
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

      {series.length === 0 && !loading ? (
        <div className="rounded-lg border border-gray-200 bg-white py-16 text-center">
          <p className="text-sm text-gray-500">No data available for the selected provider.</p>
        </div>
      ) : (
        <div className="flex flex-col gap-6">
          <section aria-labelledby="coverage-heading">
            <h2 id="coverage-heading" className="sr-only">Catalog Coverage</h2>
            <CatalogCoverageChart series={series} />
          </section>

          <section aria-labelledby="gini-heading">
            <h2 id="gini-heading" className="sr-only">Provider Gini Coefficient</h2>
            <ProviderGiniChart series={series} />
          </section>

          <section aria-labelledby="longtail-heading">
            <h2 id="longtail-heading" className="sr-only">Long-Tail Impression Share</h2>
            <LongTailImpressionChart series={series} />
          </section>
        </div>
      )}

      {result && (
        <p className="mt-4 text-xs text-gray-400" data-testid="computed-at">
          Data computed at: {new Date(result.computedAt).toLocaleString()}
        </p>
      )}
    </div>
  );
}
