'use client';

import { memo } from 'react';
import {
  LineChart,
  Line,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  ResponsiveContainer,
  Legend,
} from 'recharts';
import type { ProviderHealthSeries } from '@/lib/types';

const SERIES_COLORS = ['#4f46e5', '#10b981', '#f59e0b', '#ef4444', '#8b5cf6'];

type MetricKey = 'catalogCoverage' | 'providerGini' | 'longTailImpressionShare';

interface ProviderMetricChartProps {
  series: ProviderHealthSeries[];
  metric: MetricKey;
  title: string;
  description: string;
}

function buildChartData(
  series: ProviderHealthSeries[],
  metric: MetricKey,
): { date: string; [key: string]: string | number }[] {
  const dateMap = new Map<string, { date: string; [key: string]: string | number }>();
  for (const s of series) {
    const key = series.length > 1
      ? `${s.providerName} — ${s.experimentName}`
      : s.experimentName;
    for (const pt of s.points) {
      if (!dateMap.has(pt.date)) {
        dateMap.set(pt.date, { date: pt.date });
      }
      dateMap.get(pt.date)![key] = pt[metric];
    }
  }
  return Array.from(dateMap.values()).sort((a, b) =>
    (a.date as string).localeCompare(b.date as string),
  );
}

function pctFmt(v: number): string {
  return `${(v * 100).toFixed(1)}%`;
}

function ProviderMetricChartInner({ series, metric, title, description }: ProviderMetricChartProps) {
  if (series.length === 0) {
    return (
      <div className="rounded-lg border border-gray-200 bg-white p-6">
        <h3 className="text-sm font-semibold text-gray-900">{title}</h3>
        <p className="mt-8 text-center text-sm text-gray-500">No data available for selected provider.</p>
      </div>
    );
  }

  const seriesKeys = series.map((s) =>
    series.length > 1
      ? `${s.providerName} — ${s.experimentName}`
      : s.experimentName,
  );
  const chartData = buildChartData(series, metric);

  return (
    <div className="rounded-lg border border-gray-200 bg-white p-4">
      <div className="mb-2">
        <h3 className="text-sm font-semibold text-gray-900">{title}</h3>
        <p className="text-xs text-gray-500">{description}</p>
      </div>
      <div role="img" aria-label={`Time series: ${title}`}>
        <ResponsiveContainer width="100%" height={260}>
          <LineChart data={chartData} margin={{ top: 5, right: 16, bottom: 20, left: 4 }}>
            <CartesianGrid strokeDasharray="3 3" stroke="#e5e7eb" />
            <XAxis
              dataKey="date"
              tick={{ fontSize: 10 }}
              tickFormatter={(v: string) => v.slice(5)}
              interval="preserveStartEnd"
            />
            <YAxis
              tick={{ fontSize: 11 }}
              tickFormatter={pctFmt}
              width={52}
              domain={[0, 'auto']}
            />
            <Tooltip formatter={(v: number) => [pctFmt(v), undefined]} />
            <Legend wrapperStyle={{ fontSize: 11 }} />
            {seriesKeys.map((key, i) => (
              <Line
                key={key}
                dataKey={key}
                stroke={SERIES_COLORS[i % SERIES_COLORS.length]}
                strokeWidth={2}
                dot={false}
                connectNulls={false}
              />
            ))}
          </LineChart>
        </ResponsiveContainer>
      </div>
    </div>
  );
}

export const CatalogCoverageChart = memo(function CatalogCoverageChart(props: Omit<ProviderMetricChartProps, 'metric' | 'title' | 'description'>) {
  return (
    <ProviderMetricChartInner
      {...props}
      metric="catalogCoverage"
      title="Catalog Coverage"
      description="Fraction of provider catalog titles receiving at least one impression."
    />
  );
});

export const ProviderGiniChart = memo(function ProviderGiniChart(props: Omit<ProviderMetricChartProps, 'metric' | 'title' | 'description'>) {
  return (
    <ProviderMetricChartInner
      {...props}
      metric="providerGini"
      title="Provider Gini Coefficient"
      description="Impression concentration across provider titles. Lower is more equitable."
    />
  );
});

export const LongTailImpressionChart = memo(function LongTailImpressionChart(props: Omit<ProviderMetricChartProps, 'metric' | 'title' | 'description'>) {
  return (
    <ProviderMetricChartInner
      {...props}
      metric="longTailImpressionShare"
      title="Long-Tail Impression Share"
      description="Fraction of impressions going to titles outside the provider's top-20% by popularity."
    />
  );
});
