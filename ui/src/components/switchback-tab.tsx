'use client';

import { memo, useEffect, useState, useCallback } from 'react';
import {
  ComposedChart, Bar, BarChart, Line, XAxis, YAxis, CartesianGrid,
  Tooltip, ResponsiveContainer, ReferenceLine,
} from 'recharts';
import type { SwitchbackResult, SwitchbackBlock } from '@/lib/types';
import { getSwitchbackResult, RpcError } from '@/lib/api';
import { formatPValue } from '@/lib/utils';
import { RetryableError } from '@/components/retryable-error';

interface SwitchbackTabProps {
  experimentId: string;
}

// --- Memoised chart sub-components ---

interface AcfChartProps {
  acfPoints: SwitchbackResult['acfPoints'];
  carryoverDetected: boolean;
}

const AcfChart = memo(function AcfChart({ acfPoints, carryoverDetected }: AcfChartProps) {
  const chartData = acfPoints.map((p) => ({
    lag: `L${p.lag}`,
    acf: +p.acf.toFixed(4),
    ciUpper: +p.ciUpper.toFixed(4),
    ciLower: +p.ciLower.toFixed(4),
  }));

  return (
    <div className="rounded-lg border border-gray-200 bg-white p-4">
      <h4 className="mb-1 text-sm font-semibold text-gray-900">
        ACF — Carryover Diagnostic
        {carryoverDetected && (
          <span className="ml-2 rounded-full bg-amber-100 px-2 py-0.5 text-xs font-medium text-amber-800">
            Carryover Detected
          </span>
        )}
      </h4>
      <p className="mb-3 text-xs text-gray-500">
        Autocorrelation of residuals across lag periods. Bars outside the dashed 95% CI bounds suggest
        carryover effects between blocks.
      </p>
      <div role="img" aria-label="ACF carryover diagnostic chart">
        <ResponsiveContainer width="100%" height={220}>
          <ComposedChart data={chartData} margin={{ top: 5, right: 20, bottom: 5, left: 20 }}>
            <CartesianGrid strokeDasharray="3 3" stroke="#e5e7eb" />
            <XAxis dataKey="lag" tick={{ fontSize: 11 }} />
            <YAxis
              domain={[-1, 1]}
              tick={{ fontSize: 11 }}
              tickFormatter={(v: number) => v.toFixed(1)}
              label={{ value: 'ACF', angle: -90, position: 'insideLeft', fontSize: 12 }}
            />
            <ReferenceLine y={0} stroke="#9ca3af" />
            {/* Upper CI (dashed) — approximated as constant 1.96/sqrt(n); stored per point */}
            <Line
              type="step"
              dataKey="ciUpper"
              stroke="#f59e0b"
              strokeDasharray="4 4"
              strokeWidth={1.5}
              dot={false}
              name="CI Upper"
              isAnimationActive={false}
            />
            <Line
              type="step"
              dataKey="ciLower"
              stroke="#f59e0b"
              strokeDasharray="4 4"
              strokeWidth={1.5}
              dot={false}
              name="CI Lower"
              isAnimationActive={false}
            />
            <Bar
              dataKey="acf"
              name="ACF"
              fill="#6366f1"
              fillOpacity={0.75}
              isAnimationActive={false}
            />
            <Tooltip
              formatter={(value: number, name: string) => [
                value.toFixed(4),
                name === 'acf' ? 'Autocorrelation' :
                name === 'ciUpper' ? 'CI Upper' : 'CI Lower',
              ]}
            />
          </ComposedChart>
        </ResponsiveContainer>
      </div>
    </div>
  );
});

interface RiHistogramProps {
  nullDistribution: number[];
  riPValue: number;
}

const RiHistogram = memo(function RiHistogram({ nullDistribution, riPValue }: RiHistogramProps) {
  // Bin the null distribution into 20 equal-width buckets [0, 1]
  const nBins = 20;
  const counts = new Array<number>(nBins).fill(0);
  for (const v of nullDistribution) {
    const bin = Math.min(nBins - 1, Math.floor(v * nBins));
    counts[bin]++;
  }
  const chartData = counts.map((count, i) => ({
    bin: ((i + 0.5) / nBins).toFixed(2),
    count,
  }));

  return (
    <div className="rounded-lg border border-gray-200 bg-white p-4">
      <h4 className="mb-1 text-sm font-semibold text-gray-900">Randomization Inference — Null Distribution</h4>
      <p className="mb-3 text-xs text-gray-500">
        Distribution of ATE estimates under all valid block randomizations. The red line marks the observed
        p-value = {formatPValue(riPValue)}.
      </p>
      <div role="img" aria-label="Randomization inference null distribution histogram">
        <ResponsiveContainer width="100%" height={200}>
          <BarChart data={chartData} margin={{ top: 5, right: 20, bottom: 5, left: 20 }}>
            <CartesianGrid strokeDasharray="3 3" stroke="#e5e7eb" />
            <XAxis dataKey="bin" tick={{ fontSize: 10 }} label={{ value: 'p-value', position: 'insideBottom', offset: -2, fontSize: 11 }} />
            <YAxis tick={{ fontSize: 11 }} label={{ value: 'Count', angle: -90, position: 'insideLeft', fontSize: 11 }} />
            <ReferenceLine
              x={riPValue.toFixed(2)}
              stroke="#ef4444"
              strokeWidth={2}
              label={{ value: `p = ${formatPValue(riPValue)}`, position: 'top', fill: '#ef4444', fontSize: 10 }}
            />
            <Tooltip formatter={(value: number) => [value, 'Permutations']} />
            <Bar dataKey="count" fill="#818cf8" isAnimationActive={false} />
          </BarChart>
        </ResponsiveContainer>
      </div>
    </div>
  );
});

// --- Block timeline ---

function BlockTimeline({ blocks }: { blocks: SwitchbackBlock[] }) {
  if (blocks.length === 0) return null;

  const start = new Date(blocks[0].periodStart).getTime();
  const end = new Date(blocks[blocks.length - 1].periodEnd).getTime();
  const totalMs = end - start;

  return (
    <div className="rounded-lg border border-gray-200 bg-white p-4">
      <h4 className="mb-3 text-sm font-semibold text-gray-900">Block Timeline</h4>
      <div className="relative flex h-10 w-full overflow-hidden rounded">
        {blocks.map((block) => {
          const blockStart = new Date(block.periodStart).getTime();
          const blockEnd = new Date(block.periodEnd).getTime();
          const leftPct = ((blockStart - start) / totalMs) * 100;
          const widthPct = ((blockEnd - blockStart) / totalMs) * 100;
          const isTreatment = block.treatment === 'TREATMENT';
          return (
            <div
              key={block.blockId}
              title={`${block.blockId}: ${block.treatment}\nOutcome: ${block.outcome.toFixed(4)}\nn = ${block.n.toLocaleString()}`}
              className={`absolute h-full border-r border-white ${isTreatment ? 'bg-indigo-500' : 'bg-gray-300'}`}
              style={{ left: `${leftPct}%`, width: `${widthPct}%` }}
            />
          );
        })}
      </div>
      <div className="mt-1 flex items-center gap-4 text-xs text-gray-500">
        <span className="flex items-center gap-1">
          <span className="inline-block h-3 w-3 rounded-sm bg-indigo-500" />
          Treatment
        </span>
        <span className="flex items-center gap-1">
          <span className="inline-block h-3 w-3 rounded-sm bg-gray-300" />
          Control
        </span>
        <span className="ml-auto">
          {blocks.length} blocks total
        </span>
      </div>
    </div>
  );
}

// --- Block outcome table ---

const BlockOutcomeTable = memo(function BlockOutcomeTable({ blocks }: { blocks: SwitchbackBlock[] }) {
  return (
    <div>
      <h4 className="mb-2 text-sm font-semibold text-gray-900">Block-Level Outcomes</h4>
      <div className="overflow-hidden rounded-lg border border-gray-200 bg-white">
        <table className="min-w-full divide-y divide-gray-200">
          <thead className="bg-gray-50">
            <tr>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">Block</th>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">Period Start</th>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">Period End</th>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">Assignment</th>
              <th className="px-4 py-3 text-right text-xs font-medium uppercase tracking-wider text-gray-500">Outcome</th>
              <th className="px-4 py-3 text-right text-xs font-medium uppercase tracking-wider text-gray-500">N</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-gray-200">
            {blocks.map((block) => (
              <tr key={block.blockId} className="hover:bg-gray-50">
                <td className="whitespace-nowrap px-4 py-2 text-sm font-mono text-gray-700">{block.blockId}</td>
                <td className="whitespace-nowrap px-4 py-2 text-sm text-gray-600">
                  {new Date(block.periodStart).toLocaleDateString('en-US', { month: 'short', day: 'numeric' })}
                </td>
                <td className="whitespace-nowrap px-4 py-2 text-sm text-gray-600">
                  {new Date(block.periodEnd).toLocaleDateString('en-US', { month: 'short', day: 'numeric' })}
                </td>
                <td className="whitespace-nowrap px-4 py-2">
                  <span className={`inline-flex items-center rounded-full px-2 py-0.5 text-xs font-medium ${
                    block.treatment === 'TREATMENT'
                      ? 'bg-indigo-100 text-indigo-800'
                      : 'bg-gray-100 text-gray-700'
                  }`}>
                    {block.treatment === 'TREATMENT' ? 'Treatment' : 'Control'}
                  </span>
                </td>
                <td className="whitespace-nowrap px-4 py-2 text-right text-sm font-mono text-gray-700">
                  {block.outcome.toFixed(4)}
                </td>
                <td className="whitespace-nowrap px-4 py-2 text-right text-sm text-gray-600">
                  {block.n.toLocaleString()}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
});

// --- Main tab ---

export function SwitchbackTab({ experimentId }: SwitchbackTabProps) {
  const [result, setResult] = useState<SwitchbackResult | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const fetchData = useCallback(() => {
    setLoading(true);
    setError(null);
    getSwitchbackResult(experimentId)
      .then(setResult)
      .catch((err) => {
        if (err instanceof RpcError && err.status === 404) {
          setResult(null);
        } else {
          setError(err.message);
        }
      })
      .finally(() => setLoading(false));
  }, [experimentId]);

  useEffect(() => { fetchData(); }, [fetchData]);

  if (loading) {
    return (
      <div className="flex items-center justify-center py-8" role="status" aria-label="Loading">
        <div className="h-6 w-6 animate-spin rounded-full border-4 border-gray-300 border-t-indigo-600" />
        <span className="sr-only">Loading</span>
      </div>
    );
  }

  if (error) {
    return <RetryableError message={error} onRetry={fetchData} context="switchback analysis" />;
  }

  if (!result) {
    return (
      <div className="rounded-md bg-gray-50 p-4 text-sm text-gray-500">
        No switchback analysis available for this experiment.
      </div>
    );
  }

  const ateSign = result.ate >= 0 ? '+' : '';
  const isSignificant = result.riPValue < 0.05;

  return (
    <div className="space-y-6">
      {/* Significance banner */}
      <div className={`rounded-md border p-4 ${isSignificant ? 'bg-green-50 border-green-200' : 'bg-gray-50 border-gray-200'}`}>
        <h4 className={`text-sm font-semibold ${isSignificant ? 'text-green-800' : 'text-gray-800'}`}>
          Switchback ATE — {result.metricId}
          {isSignificant && (
            <span className="ml-2 rounded-full bg-green-200 px-2 py-0.5 text-xs font-medium text-green-900">
              Significant
            </span>
          )}
        </h4>
        <div className="mt-2 flex flex-wrap gap-6">
          <div>
            <span className="text-2xl font-bold text-gray-900">{ateSign}{result.ate.toFixed(4)}</span>
            <span className="ml-2 text-sm text-gray-500">
              95% CI [{result.ateCiLower.toFixed(4)}, {result.ateCiUpper.toFixed(4)}]
            </span>
          </div>
          <div className="flex flex-col">
            <span className="text-xs font-medium uppercase text-gray-500">RI p-value</span>
            <span className={`text-lg font-semibold ${isSignificant ? 'text-green-700' : 'text-gray-700'}`}>
              {formatPValue(result.riPValue)}
            </span>
          </div>
          <div className="flex flex-col">
            <span className="text-xs font-medium uppercase text-gray-500">SE</span>
            <span className="text-lg font-semibold text-gray-700">{result.ateSe.toFixed(4)}</span>
          </div>
          <div className="flex flex-col">
            <span className="text-xs font-medium uppercase text-gray-500">Blocks</span>
            <span className="text-lg font-semibold text-gray-700">
              {result.nTreatmentBlocks}T / {result.nControlBlocks}C
            </span>
          </div>
        </div>
      </div>

      {/* Block timeline */}
      <BlockTimeline blocks={result.blocks} />

      {/* Block outcome table */}
      <BlockOutcomeTable blocks={result.blocks} />

      {/* ACF carryover plot */}
      {result.acfPoints.length > 0 && (
        <AcfChart acfPoints={result.acfPoints} carryoverDetected={result.carryoverDetected} />
      )}

      {/* RI p-value histogram */}
      {result.riNullDistribution.length > 0 && (
        <RiHistogram nullDistribution={result.riNullDistribution} riPValue={result.riPValue} />
      )}
    </div>
  );
}
