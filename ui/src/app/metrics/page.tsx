'use client';

import { useEffect, useState, useCallback, useMemo, useRef } from 'react';
import dynamic from 'next/dynamic';
import { useSearchShortcut } from '@/hooks/use-search-shortcut';
import Link from 'next/link';
import type { MetricDefinition, MetricType } from '@/lib/types';
import { CompositeOperator } from '@/lib/types';
import { listMetricDefinitions } from '@/lib/api';
import { RetryableError } from '@/components/retryable-error';
import { CopyButton } from '@/components/copy-button';
import { Breadcrumb } from '@/components/breadcrumb';
import { useAuth } from '@/lib/auth-context';
import { ROLE_LABELS } from '@/lib/auth';

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
  // ADR-026 Phase 1 — colors per plan: teal / indigo / rose.
  FILTERED_MEAN: 'bg-teal-100 text-teal-800',
  COMPOSITE: 'bg-indigo-100 text-indigo-800',
  WINDOWED_COUNT: 'bg-rose-100 text-rose-800',
  // ADR-026 Phase 2 — violet matches the MetricqlSection fieldset border color.
  METRICQL: 'bg-violet-100 text-violet-800',
};

const COMPOSITE_OPERATOR_NAME: Record<CompositeOperator, string> = {
  [CompositeOperator.UNSPECIFIED]: 'UNSPECIFIED',
  [CompositeOperator.ADD]: 'ADD',
  [CompositeOperator.SUBTRACT]: 'SUBTRACT',
  [CompositeOperator.MULTIPLY]: 'MULTIPLY',
  [CompositeOperator.DIVIDE]: 'DIVIDE',
  [CompositeOperator.WEIGHTED_SUM]: 'WEIGHTED_SUM',
};

/**
 * Inline detail-row renderer for the ADR-026 Phase 1 `typeConfig` oneof.
 * Kept inline (not extracted) — the existing detail row is hand-rolled and
 * this block follows the same `<dl><div><dt>/<dd></div></dl>` pattern.
 */
function TypeConfigDetail({ metric }: { metric: MetricDefinition }) {
  const cfg = metric.typeConfig;
  if (!cfg) return null;
  switch (cfg.case) {
    case 'filteredMean':
      return (
        <>
          <div>
            <dt className="font-medium text-gray-500">Value Column</dt>
            <dd className="text-gray-900"><code className="text-xs">{cfg.value.valueColumn}</code></dd>
          </div>
          <div className="col-span-2">
            <dt className="font-medium text-gray-500">Filter SQL</dt>
            <dd className="mt-1">
              <SqlHighlighter sql={cfg.value.filterSql} />
            </dd>
          </div>
        </>
      );
    case 'composite':
      return (
        <>
          <div>
            <dt className="font-medium text-gray-500">Operator</dt>
            <dd className="text-gray-900">{COMPOSITE_OPERATOR_NAME[cfg.value.operator]}</dd>
          </div>
          <div className="col-span-2">
            <dt className="font-medium text-gray-500">Operands</dt>
            <dd className="mt-1">
              <ul className="list-disc pl-5 text-gray-900">
                {cfg.value.operands.map((op, idx) => (
                  <li key={`${op.metricId}-${idx}`} data-testid={`composite-operand-${idx}`}>
                    <code className="text-xs">{op.metricId}</code>
                    {cfg.value.operator === CompositeOperator.WEIGHTED_SUM && (
                      <span className="ml-2 text-xs text-gray-500">weight: {op.weight}</span>
                    )}
                  </li>
                ))}
              </ul>
            </dd>
          </div>
        </>
      );
    case 'windowedCount':
      return (
        <>
          <div>
            <dt className="font-medium text-gray-500">Event Type</dt>
            <dd className="text-gray-900"><code className="text-xs">{cfg.value.eventType}</code></dd>
          </div>
          <div>
            <dt className="font-medium text-gray-500">Window (hours)</dt>
            <dd className="text-gray-900">{cfg.value.windowHours}</dd>
          </div>
          {cfg.value.filterSql && (
            <div className="col-span-2">
              <dt className="font-medium text-gray-500">Filter SQL</dt>
              <dd className="mt-1">
                <SqlHighlighter sql={cfg.value.filterSql} />
              </dd>
            </div>
          )}
        </>
      );
  }
}

const ALL_METRIC_TYPES: MetricType[] = [
  'MEAN',
  'PROPORTION',
  'RATIO',
  'COUNT',
  'PERCENTILE',
  'CUSTOM',
  // ADR-026 Phase 1
  'FILTERED_MEAN',
  'COMPOSITE',
  'WINDOWED_COUNT',
  // ADR-026 Phase 2
  'METRICQL',
];

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
        className="group cursor-pointer hover:bg-gray-50 focus-within:bg-gray-50 focus-within:outline-none focus-within:ring-2 focus-within:ring-inset focus-within:ring-indigo-500"
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
              className="h-4 w-4 opacity-0 group-hover:opacity-100 group-focus-within:opacity-100 transition-opacity"
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
              <TypeConfigDetail metric={metric} />
            </dl>
          </td>
        </tr>
      )}
    </>
  );
}

function MetricBrowserContent() {
  const { canAtLeast, user } = useAuth();
  const [metrics, setMetrics] = useState<MetricDefinition[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [search, setSearch] = useState('');
  const [typeFilter, setTypeFilter] = useState<MetricType | ''>('');
  const inputRef = useRef<HTMLInputElement>(null);
  useSearchShortcut(inputRef);

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

  const isEmpty = metrics.length === 0;

  return (
    <div>
      <Breadcrumb items={[
        { label: 'Experiments', href: '/' },
        { label: 'Metrics' },
      ]} />

      <div className="mb-6 flex items-center justify-between">
        <div className="flex items-center gap-3">
          <h1 className="text-2xl font-bold text-gray-900">Metric Definitions</h1>
          {!loading && !error && !isEmpty && (
            <span className="inline-flex items-center rounded-full bg-gray-100 px-2.5 py-0.5 text-xs font-medium text-gray-700" data-testid="metric-count">
              {filtered.length}
            </span>
          )}
        </div>
        {!loading && !error && (
          canAtLeast('experimenter') ? (
            <Link
              href="/metrics/new"
              className="rounded-md bg-indigo-600 px-3 py-2 text-sm font-medium text-white shadow-sm hover:bg-indigo-500 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500 focus-visible:ring-offset-2"
              data-testid="new-metric-button"
            >
              New Metric
            </Link>
          ) : (
            <span
              className="rounded-md bg-indigo-600 px-3 py-2 text-sm font-medium text-white opacity-50 cursor-not-allowed focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500 focus-visible:ring-offset-2"
              title={`Requires Experimenter role (you are ${ROLE_LABELS[user.role]})`}
              data-testid="new-metric-disabled"
              tabIndex={0}
              role="button"
              aria-disabled="true"
            >
              New Metric
            </span>
          )
        )}
      </div>

      {loading ? (
        <div className="flex items-center justify-center py-12" role="status" aria-label="Loading">
          <div className="h-8 w-8 animate-spin rounded-full border-4 border-gray-300 border-t-indigo-600" />
          <span className="sr-only">Loading</span>
        </div>
      ) : error ? (
        <RetryableError message={error} onRetry={fetchData} context="metric definitions" />
      ) : isEmpty ? (
        <div className="py-12 text-center" data-testid="empty-state">
          <p className="text-sm text-gray-500">No metric definitions found.</p>
          {canAtLeast('experimenter') && (
            <div className="mt-6">
              <Link
                href="/metrics/new"
                className="rounded-md bg-indigo-600 px-3 py-2 text-sm font-medium text-white shadow-sm hover:bg-indigo-500 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500 focus-visible:ring-offset-2"
                data-testid="create-first-metric"
              >
                Create your first metric
              </Link>
            </div>
          )}
        </div>
      ) : (
        <>

      <div className="mb-4 flex flex-wrap items-center gap-3">
        <div className="group relative flex-1 min-w-[300px] max-w-md">
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
            ref={inputRef}
            type="text"
            placeholder="Search by name, ID, or description..."
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            className="w-full rounded-md border border-gray-300 py-1.5 pl-9 pr-10 text-sm shadow-sm placeholder:text-gray-400 focus:border-indigo-500 focus:outline-none focus:ring-1 focus:ring-indigo-500"
            data-testid="metric-search"
            aria-label="Search metrics"
          />
          {search ? (
            <button
              type="button"
              onClick={() => setSearch('')}
              className="absolute right-3 top-1/2 -translate-y-1/2 text-gray-400 hover:text-gray-600 focus:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500 rounded-sm"
              aria-label="Clear search"
              data-testid="clear-search-button"
            >
              <svg className="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" aria-hidden="true">
                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M6 18L18 6M6 6l12 12" />
              </svg>
            </button>
          ) : (
            <div className="pointer-events-none absolute right-3 top-1/2 flex -translate-y-1/2 items-center group-focus-within:hidden group-hover:hidden">
              <span className="flex h-5 w-5 items-center justify-center rounded border border-gray-300 bg-gray-50 text-[10px] font-medium text-gray-500">
                /
              </span>
            </div>
          )}
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
            className="rounded-md border border-gray-300 px-3 py-1.5 text-sm text-gray-600 hover:bg-gray-50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500 focus-visible:ring-offset-2"
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
              className="mt-2 rounded-sm text-sm text-indigo-600 hover:text-indigo-800 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500 focus-visible:ring-offset-2"
              data-testid="clear-filters-empty"
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
        </>
      )}
    </div>
  );
}

export default function MetricBrowserPage() {
  return <MetricBrowserContent />;
}
