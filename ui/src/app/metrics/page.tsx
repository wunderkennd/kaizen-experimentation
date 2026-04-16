'use client';

import { useEffect, useState, useCallback, useMemo } from 'react';
import dynamic from 'next/dynamic';
import type { MetricDefinition, MetricType } from '@/lib/types';
import { listMetricDefinitions } from '@/lib/api';
import { RetryableError } from '@/components/retryable-error';
import { CopyButton } from '@/components/copy-button';

const SqlHighlighter = dynamic(
  () => import('@/components/sql-highlighter').then((m) => ({ default: m.SqlHighlighter })),
  {
    ssr: false,
    loading: () => (
      <pre className="mt-2 animate-pulse rounded bg-gray-50 p-3 font-mono text-xs">Loading...</pre>
    ),
  },
);

const METRIC_TYPE_BADGE: Record<MetricType, string> = {
  MEAN: 'bg-blue-100 text-blue-800',
  PROPORTION: 'bg-green-100 text-green-800',
  RATIO: 'bg-purple-100 text-purple-800',
  COUNT: 'bg-gray-100 text-gray-800',
  PERCENTILE: 'bg-amber-100 text-amber-800',
  CUSTOM: 'bg-orange-100 text-orange-800',
};

const ALL_METRIC_TYPES: MetricType[] = ['MEAN', 'PROPORTION', 'RATIO', 'COUNT', 'PERCENTILE', 'CUSTOM'];

function MetricRow({ metric }: { metric: MetricDefinition }) {
  const [expanded, setExpanded] = useState(false);

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      setExpanded(!expanded);
    }
  };

  return (
    <>
      <tr
        className="cursor-pointer hover:bg-gray-50 focus:outline-none focus:ring-2 focus:ring-inset focus:ring-indigo-500"
        onClick={() => setExpanded(!expanded)}
        onKeyDown={handleKeyDown}
        tabIndex={0}
        role="button"
        aria-expanded={expanded}
        aria-label={`Toggle details for ${metric.name}`}
        data-testid={`metric-row-${metric.metricId}`}
      >
        <td className="px-4 py-3">
          <div className="flex items-center gap-2">
            <svg
              className={`h-4 w-4 text-gray-400 transition-transform ${expanded ? 'rotate-90' : ''}`}
              fill="none"
              viewBox="0 0 24 24"
              stroke="currentColor"
              aria-hidden="true"
            >
              <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M9 5l7 7-7 7" />
            </svg>
            <span className="font-medium text-gray-900">{metric.name}</span>
          </div>
        </td>
        <td className="px-4 py-3">
          <div className="flex items-center gap-2">
            <code className="text-xs text-gray-500">{metric.metricId}</code>
            <CopyButton
              value={metric.metricId}
              label="Copy metric ID"
              successMessage="Metric ID copied"
              className="h-4 w-4"
            />
          </div>
        </td>
        <td className="px-4 py-3">
          <span
            className={`inline-flex rounded-full px-2 py-0.5 text-xs font-medium ${METRIC_TYPE_BADGE[metric.type]}`}
            data-testid={`type-badge-${metric.metricId}`}
          >
            {metric.type}
          </span>
        </td>
        <td className="px-4 py-3 text-sm text-gray-600">{metric.sourceEventType}</td>
        <td className="px-4 py-3 text-sm">
          <span data-testid={`direction-${metric.metricId}`}>
            {metric.lowerIsBetter ? '↓ lower is better' : '↑ higher is better'}
          </span>
        </td>
        <td className="px-4 py-3">
          <div className="flex gap-1">
            {metric.isQoeMetric && (
              <span className="inline-flex rounded-full bg-pink-100 px-2 py-0.5 text-xs font-medium text-pink-700" data-testid={`qoe-badge-${metric.metricId}`}>
                QoE
              </span>
            )}
            {metric.surrogateTargetMetricId && (
              <span className="inline-flex rounded-full bg-indigo-100 px-2 py-0.5 text-xs font-medium text-indigo-700">
                Surrogate
              </span>
            )}
            {metric.cupedCovariateMetricId && (
              <span className="inline-flex rounded-full bg-teal-100 px-2 py-0.5 text-xs font-medium text-teal-700">
                CUPED
              </span>
            )}
          </div>
        </td>
      </tr>
      {expanded && (
        <tr data-testid={`metric-detail-${metric.metricId}`}>
          <td colSpan={6} className="bg-gray-50 px-4 py-3">
            <dl className="grid grid-cols-2 gap-x-8 gap-y-2 text-sm">
              <div>
                <dt className="font-medium text-gray-500">Description</dt>
                <dd className="text-gray-900">{metric.description}</dd>
              </div>
              {metric.numeratorEventType && (
                <div>
                  <dt className="font-medium text-gray-500">Numerator Event</dt>
                  <dd className="text-gray-900">{metric.numeratorEventType}</dd>
                </div>
              )}
              {metric.denominatorEventType && (
                <div>
                  <dt className="font-medium text-gray-500">Denominator Event</dt>
                  <dd className="text-gray-900">{metric.denominatorEventType}</dd>
                </div>
              )}
              {metric.percentile != null && (
                <div>
                  <dt className="font-medium text-gray-500">Percentile</dt>
                  <dd className="text-gray-900">p{metric.percentile}</dd>
                </div>
              )}
              {metric.customSql && (
                <div className="col-span-2">
                  <dt className="font-medium text-gray-500">Custom SQL</dt>
                  <dd className="mt-1">
                    <SqlHighlighter sql={metric.customSql} />
                  </dd>
                </div>
              )}
              {metric.minimumDetectableEffect != null && (
                <div>
                  <dt className="font-medium text-gray-500">MDE</dt>
                  <dd className="text-gray-900">{(metric.minimumDetectableEffect * 100).toFixed(1)}%</dd>
                </div>
              )}
              {metric.cupedCovariateMetricId && (
                <div>
                  <dt className="font-medium text-gray-500">CUPED Covariate</dt>
                  <dd className="mt-1 flex items-center gap-2 text-gray-900">
                    <code className="text-xs">{metric.cupedCovariateMetricId}</code>
                    <CopyButton
                      value={metric.cupedCovariateMetricId}
                      label="Copy CUPED covariate ID"
                      successMessage="CUPED covariate ID copied"
                      className="h-4 w-4"
                    />
                  </dd>
                </div>
              )}
              {metric.surrogateTargetMetricId && (
                <div>
                  <dt className="font-medium text-gray-500">Surrogate Target</dt>
                  <dd className="mt-1 flex items-center gap-2 text-gray-900">
                    <code className="text-xs">{metric.surrogateTargetMetricId}</code>
                    <CopyButton
                      value={metric.surrogateTargetMetricId}
                      label="Copy surrogate target ID"
                      successMessage="Surrogate target ID copied"
                      className="h-4 w-4"
                    />
                  </dd>
                </div>
              )}
            </dl>
          </td>
        </tr>
      )}
    </>
  );
}

function MetricBrowserContent() {
  const [metrics, setMetrics] = useState<MetricDefinition[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [search, setSearch] = useState('');
  const [typeFilter, setTypeFilter] = useState<MetricType | ''>('');

  const hasActiveFilters = search !== '' || typeFilter !== '';

  const clearFilters = useCallback(() => {
    setSearch('');
    setTypeFilter('');
  }, []);

  const fetchData = useCallback(() => {
    setLoading(true);
    setError(null);
    listMetricDefinitions()
      .then((data) => {
        setMetrics(data.metrics);
      })
      .catch((err) => {
        setError(err.message);
      })
      .finally(() => {
        setLoading(false);
      });
  }, []);

  useEffect(() => { fetchData(); }, [fetchData]);

  const filtered = useMemo(() => {
    let result = metrics;
    if (typeFilter) {
      result = result.filter((m) => m.type === typeFilter);
    }
    if (search) {
      const q = search.toLowerCase();
      result = result.filter(
        (m) =>
          m.name.toLowerCase().includes(q) ||
          m.description.toLowerCase().includes(q) ||
          m.metricId.toLowerCase().includes(q),
      );
    }
    return result;
  }, [metrics, typeFilter, search]);

  if (loading) {
    return (
      <div className="flex items-center justify-center py-12" role="status" aria-label="Loading">
        <div className="h-8 w-8 animate-spin rounded-full border-4 border-gray-300 border-t-indigo-600" />
        <span className="sr-only">Loading</span>
      </div>
    );
  }

  if (error) {
    return <RetryableError message={error} onRetry={fetchData} context="metric definitions" />;
  }

  if (metrics.length === 0) {
    return (
      <div className="py-12 text-center" data-testid="empty-state">
        <p className="text-sm text-gray-500">No metric definitions found.</p>
      </div>
    );
  }

  return (
    <div>
      <div className="mb-6 flex items-center gap-3">
        <h1 className="text-2xl font-bold text-gray-900">Metric Definitions</h1>
        <span className="inline-flex items-center rounded-full bg-gray-100 px-2.5 py-0.5 text-xs font-medium text-gray-700" data-testid="metric-count">
          {filtered.length}
        </span>
      </div>

      <div className="mb-4 flex flex-wrap items-center gap-3">
        <div className="relative flex-1 min-w-[300px] max-w-md">
          <svg
            className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-gray-400"
            fill="none"
            stroke="currentColor"
            viewBox="0 0 24 24"
            aria-hidden="true"
          >
            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z" />
          </svg>
          <input
            type="text"
            placeholder="Search by name, ID, or description..."
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            className="w-full rounded-md border border-gray-300 py-1.5 pl-9 pr-3 text-sm shadow-sm placeholder:text-gray-400 focus:border-indigo-500 focus:outline-none focus:ring-1 focus:ring-indigo-500"
            data-testid="metric-search"
            aria-label="Search metrics"
          />
        </div>
        <select
          value={typeFilter}
          onChange={(e) => setTypeFilter(e.target.value as MetricType | '')}
          className="rounded-md border border-gray-300 px-3 py-1.5 text-sm shadow-sm focus:border-indigo-500 focus:outline-none focus:ring-1 focus:ring-indigo-500"
          data-testid="type-filter"
          aria-label="Filter by metric type"
        >
          <option value="">All Types</option>
          {ALL_METRIC_TYPES.map((t) => (
            <option key={t} value={t}>{t}</option>
          ))}
        </select>
        {hasActiveFilters && (
          <button
            onClick={clearFilters}
            className="rounded-md border border-gray-300 px-3 py-1.5 text-sm text-gray-600 hover:bg-gray-50"
            data-testid="clear-filters-toolbar"
          >
            Clear filters
          </button>
        )}
      </div>

      {filtered.length === 0 ? (
        <div className="py-12 text-center" data-testid="no-filter-matches">
          <p className="text-sm text-gray-500">No metrics match your filters.</p>
          <button
            onClick={clearFilters}
            className="mt-2 text-sm text-indigo-600 hover:text-indigo-800"
          >
            Clear filters
          </button>
        </div>
      ) : (
        <div className="overflow-hidden rounded-lg border border-gray-200 bg-white shadow-sm">
          <table className="min-w-full divide-y divide-gray-200">
            <thead className="bg-gray-50">
              <tr>
                <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">Name</th>
                <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">Metric ID</th>
                <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">Type</th>
                <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">Source Event</th>
                <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">Direction</th>
                <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">Flags</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-gray-200">
              {filtered.map((m) => (
                <MetricRow key={m.metricId} metric={m} />
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}

export default function MetricBrowserPage() {
  return <MetricBrowserContent />;
}
