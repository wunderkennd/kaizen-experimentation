'use client';

import { memo, useEffect, useState, useCallback } from 'react';
import {
  ComposedChart, Line, Area, XAxis, YAxis, CartesianGrid,
  Tooltip, ResponsiveContainer, ReferenceLine, Legend,
} from 'recharts';
import type { SyntheticControlResult, PlaceboResult } from '@/lib/types';
import { getSyntheticControlResult, RpcError } from '@/lib/api';
import { formatPValue } from '@/lib/utils';
import { RetryableError } from '@/components/retryable-error';

interface QuasiExperimentTabProps {
  experimentId: string;
}

// --- Memoised chart components ---

interface TreatedVsSyntheticChartProps {
  timeSeries: SyntheticControlResult['timeSeries'];
  treatmentStartDate: string;
}

const TreatedVsSyntheticChart = memo(function TreatedVsSyntheticChart({
  timeSeries,
  treatmentStartDate,
}: TreatedVsSyntheticChartProps) {
  const data = timeSeries.map((p) => ({
    date: p.date.slice(5), // MM-DD
    treated: p.treated,
    synthetic: p.synthetic,
    ciLower: p.ciLower,
    ciUpper: p.ciUpper,
  }));

  return (
    <div className="rounded-lg border border-gray-200 bg-white p-4">
      <h4 className="mb-1 text-sm font-semibold text-gray-900">Treated vs Synthetic Control</h4>
      <p className="mb-3 text-xs text-gray-500">
        Dashed vertical line marks treatment start ({treatmentStartDate.slice(0, 10)}).
        Shaded band shows 95% confidence interval on the synthetic counterfactual.
      </p>
      <div role="img" aria-label="Treated vs synthetic control time series chart">
        <ResponsiveContainer width="100%" height={280}>
          <ComposedChart data={data} margin={{ top: 5, right: 20, bottom: 5, left: 30 }}>
            <CartesianGrid strokeDasharray="3 3" stroke="#e5e7eb" />
            <XAxis dataKey="date" tick={{ fontSize: 10 }} interval="preserveStartEnd" />
            <YAxis tick={{ fontSize: 11 }} tickFormatter={(v: number) => v.toFixed(3)} />
            <ReferenceLine
              x={treatmentStartDate.slice(5, 10)}
              stroke="#dc2626"
              strokeDasharray="6 3"
              label={{ value: 'Treatment', position: 'top', fill: '#dc2626', fontSize: 10 }}
            />
            <Tooltip
              labelFormatter={(label: string) => `Date: ${label}`}
              formatter={(value: number | undefined, name: string) => [
                value !== undefined ? value.toFixed(4) : '—',
                name === 'treated' ? 'Treated' :
                name === 'synthetic' ? 'Synthetic Control' :
                name === 'ciUpper' ? 'CI Upper' : 'CI Lower',
              ]}
            />
            <Legend
              formatter={(value: string) =>
                value === 'treated' ? 'Treated' :
                value === 'synthetic' ? 'Synthetic Control' : value
              }
            />
            {/* Confidence band */}
            <Area
              dataKey="ciUpper"
              stroke="none"
              fill="#c7d2fe"
              fillOpacity={0.4}
              name="CI Upper"
              legendType="none"
              isAnimationActive={false}
            />
            <Area
              dataKey="ciLower"
              stroke="none"
              fill="#ffffff"
              fillOpacity={1}
              name="CI Lower"
              legendType="none"
              isAnimationActive={false}
            />
            <Line
              type="monotone"
              dataKey="synthetic"
              stroke="#6b7280"
              strokeWidth={2}
              strokeDasharray="5 3"
              dot={false}
              name="synthetic"
              isAnimationActive={false}
            />
            <Line
              type="monotone"
              dataKey="treated"
              stroke="#6366f1"
              strokeWidth={2}
              dot={false}
              name="treated"
              isAnimationActive={false}
            />
          </ComposedChart>
        </ResponsiveContainer>
      </div>
    </div>
  );
});

interface EffectsChartProps {
  effects: SyntheticControlResult['effects'];
  treatmentStartDate: string;
}

const PointwiseEffectsChart = memo(function PointwiseEffectsChart({
  effects,
  treatmentStartDate,
}: EffectsChartProps) {
  const data = effects.map((p) => ({
    date: p.date.slice(5),
    effect: p.pointwiseEffect,
  }));

  return (
    <div className="rounded-lg border border-gray-200 bg-white p-4">
      <h4 className="mb-1 text-sm font-semibold text-gray-900">Pointwise Treatment Effects</h4>
      <p className="mb-3 text-xs text-gray-500">
        Treated minus synthetic at each time period after treatment onset.
      </p>
      <div role="img" aria-label="Pointwise treatment effects chart">
        <ResponsiveContainer width="100%" height={220}>
          <ComposedChart data={data} margin={{ top: 5, right: 20, bottom: 5, left: 30 }}>
            <CartesianGrid strokeDasharray="3 3" stroke="#e5e7eb" />
            <XAxis dataKey="date" tick={{ fontSize: 10 }} interval="preserveStartEnd" />
            <YAxis tick={{ fontSize: 11 }} tickFormatter={(v: number) => v.toFixed(3)} />
            <ReferenceLine y={0} stroke="#9ca3af" strokeDasharray="4 4" />
            <ReferenceLine
              x={treatmentStartDate.slice(5, 10)}
              stroke="#dc2626"
              strokeDasharray="6 3"
            />
            <Tooltip
              formatter={(value: number) => [value.toFixed(4), 'Pointwise Effect']}
              labelFormatter={(label: string) => `Date: ${label}`}
            />
            <Line
              type="monotone"
              dataKey="effect"
              stroke="#6366f1"
              strokeWidth={2}
              dot={{ r: 3 }}
              name="Pointwise Effect"
              isAnimationActive={false}
            />
          </ComposedChart>
        </ResponsiveContainer>
      </div>
    </div>
  );
});

const CumulativeEffectsChart = memo(function CumulativeEffectsChart({
  effects,
  treatmentStartDate,
}: EffectsChartProps) {
  const data = effects.map((p) => ({
    date: p.date.slice(5),
    cumulative: p.cumulativeEffect,
  }));

  return (
    <div className="rounded-lg border border-gray-200 bg-white p-4">
      <h4 className="mb-1 text-sm font-semibold text-gray-900">Cumulative Treatment Effect</h4>
      <p className="mb-3 text-xs text-gray-500">
        Running sum of pointwise effects since treatment onset.
      </p>
      <div role="img" aria-label="Cumulative treatment effects chart">
        <ResponsiveContainer width="100%" height={220}>
          <ComposedChart data={data} margin={{ top: 5, right: 20, bottom: 5, left: 30 }}>
            <CartesianGrid strokeDasharray="3 3" stroke="#e5e7eb" />
            <XAxis dataKey="date" tick={{ fontSize: 10 }} interval="preserveStartEnd" />
            <YAxis tick={{ fontSize: 11 }} tickFormatter={(v: number) => v.toFixed(3)} />
            <ReferenceLine y={0} stroke="#9ca3af" strokeDasharray="4 4" />
            <ReferenceLine
              x={treatmentStartDate.slice(5, 10)}
              stroke="#dc2626"
              strokeDasharray="6 3"
            />
            <Tooltip
              formatter={(value: number) => [value.toFixed(4), 'Cumulative Effect']}
              labelFormatter={(label: string) => `Date: ${label}`}
            />
            <Line
              type="monotone"
              dataKey="cumulative"
              stroke="#16a34a"
              strokeWidth={2}
              dot={false}
              name="Cumulative Effect"
              isAnimationActive={false}
            />
          </ComposedChart>
        </ResponsiveContainer>
      </div>
    </div>
  );
});

// --- Placebo mini chart (one per donor) ---

interface PlaceboMiniChartProps {
  placebo: PlaceboResult;
  treatedEffects: SyntheticControlResult['effects'];
}

const PlaceboMiniChart = memo(function PlaceboMiniChart({ placebo, treatedEffects }: PlaceboMiniChartProps) {
  // Align placebo series with treated effects by date
  const treatedByDate = new Map(treatedEffects.map((e) => [e.date.slice(5), e.pointwiseEffect]));
  const data = placebo.series.map((p) => ({
    date: p.date.slice(5),
    placeboEffect: p.effect,
    treatedEffect: treatedByDate.get(p.date.slice(5)) ?? null,
  }));

  return (
    <div className="rounded-lg border border-gray-100 bg-gray-50 p-3">
      <div className="mb-1 flex items-center justify-between">
        <span className="text-xs font-semibold text-gray-700 truncate">{placebo.donorName}</span>
        <span className={`ml-1 shrink-0 rounded px-1.5 py-0.5 text-xs font-medium ${
          placebo.rmspeRatio > 4
            ? 'bg-red-100 text-red-700'
            : placebo.rmspeRatio > 2
            ? 'bg-amber-100 text-amber-700'
            : 'bg-green-100 text-green-700'
        }`}>
          {placebo.rmspeRatio.toFixed(2)}×
        </span>
      </div>
      <div role="img" aria-label={`Placebo chart for ${placebo.donorName}`}>
        <ResponsiveContainer width="100%" height={100}>
          <ComposedChart data={data} margin={{ top: 2, right: 4, bottom: 2, left: 4 }}>
            <ReferenceLine y={0} stroke="#d1d5db" />
            <XAxis dataKey="date" hide />
            <YAxis hide />
            <Tooltip
              formatter={(value: number | null, name: string) => [
                value !== null ? value.toFixed(4) : '—',
                name === 'treatedEffect' ? 'Treated' : 'Placebo',
              ]}
            />
            <Line
              type="monotone"
              dataKey="treatedEffect"
              stroke="#6366f1"
              strokeWidth={1.5}
              strokeDasharray="3 2"
              dot={false}
              name="treatedEffect"
              isAnimationActive={false}
            />
            <Line
              type="monotone"
              dataKey="placeboEffect"
              stroke="#9ca3af"
              strokeWidth={1.5}
              dot={false}
              name="placeboEffect"
              isAnimationActive={false}
            />
          </ComposedChart>
        </ResponsiveContainer>
      </div>
      <div className="mt-1 flex justify-between text-xs text-gray-400">
        <span>Pre-RMSPE: {placebo.preRmspe.toFixed(4)}</span>
        <span>Post: {placebo.postRmspe.toFixed(4)}</span>
      </div>
    </div>
  );
});

// --- RMSPE Diagnostic Badge ---

function RmspeBadge({ preRmspe, postRmspe, rmspeRatio, pValue }: {
  preRmspe: number;
  postRmspe: number;
  rmspeRatio: number;
  pValue: number;
}) {
  const isSignificant = pValue < 0.05;
  const quality =
    rmspeRatio > 4 ? { label: 'Poor Fit', color: 'bg-red-50 border-red-200 text-red-800' } :
    rmspeRatio > 2 ? { label: 'Moderate Fit', color: 'bg-amber-50 border-amber-200 text-amber-800' } :
    { label: 'Good Fit', color: 'bg-green-50 border-green-200 text-green-800' };

  return (
    <div className={`rounded-lg border p-4 ${quality.color}`}>
      <div className="flex flex-wrap items-start justify-between gap-4">
        <div>
          <h4 className="text-sm font-semibold">RMSPE Diagnostic — {quality.label}</h4>
          <p className="mt-1 text-xs opacity-80">
            Pre-treatment fit quality relative to post-treatment deviation.
            Low pre-RMSPE indicates a good synthetic match. High ratio suggests a strong causal effect.
          </p>
        </div>
        <span className={`rounded-full px-3 py-1 text-sm font-bold ${
          isSignificant ? 'bg-green-200 text-green-900' : 'bg-gray-200 text-gray-700'
        }`}>
          p = {formatPValue(pValue)}
        </span>
      </div>
      <div className="mt-3 grid grid-cols-3 gap-3">
        <div className="rounded bg-white bg-opacity-60 p-2 text-center">
          <div className="text-xs font-medium uppercase opacity-70">Pre-RMSPE</div>
          <div className="mt-0.5 text-lg font-bold">{preRmspe.toFixed(4)}</div>
        </div>
        <div className="rounded bg-white bg-opacity-60 p-2 text-center">
          <div className="text-xs font-medium uppercase opacity-70">Post-RMSPE</div>
          <div className="mt-0.5 text-lg font-bold">{postRmspe.toFixed(4)}</div>
        </div>
        <div className="rounded bg-white bg-opacity-60 p-2 text-center">
          <div className="text-xs font-medium uppercase opacity-70">Ratio</div>
          <div className="mt-0.5 text-lg font-bold">{rmspeRatio.toFixed(2)}×</div>
        </div>
      </div>
    </div>
  );
}

// --- Donor Weight Table ---

const DonorWeightTable = memo(function DonorWeightTable({
  donorWeights,
}: {
  donorWeights: SyntheticControlResult['donorWeights'];
}) {
  const sorted = [...donorWeights].sort((a, b) => b.weight - a.weight);

  return (
    <div>
      <h4 className="mb-2 text-sm font-semibold text-gray-900">Donor Weights</h4>
      <div className="overflow-hidden rounded-lg border border-gray-200 bg-white">
        <table className="min-w-full divide-y divide-gray-200">
          <thead className="bg-gray-50">
            <tr>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">Donor</th>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">Weight</th>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">Contribution</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-gray-200">
            {sorted.map((d) => (
              <tr key={d.donorId} className="hover:bg-gray-50">
                <td className="whitespace-nowrap px-4 py-2 text-sm text-gray-900">{d.donorName}</td>
                <td className="whitespace-nowrap px-4 py-2 text-sm font-mono text-gray-700">
                  {d.weight.toFixed(4)}
                </td>
                <td className="px-4 py-2">
                  <div className="flex items-center gap-2">
                    <div className="flex-1 h-3 rounded bg-gray-100 overflow-hidden">
                      <div
                        className="h-full rounded bg-indigo-500"
                        style={{ width: `${(d.weight * 100).toFixed(1)}%` }}
                      />
                    </div>
                    <span className="w-12 text-right text-xs text-gray-500">
                      {(d.weight * 100).toFixed(1)}%
                    </span>
                  </div>
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

export function QuasiExperimentTab({ experimentId }: QuasiExperimentTabProps) {
  const [result, setResult] = useState<SyntheticControlResult | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const fetchData = useCallback(() => {
    setLoading(true);
    setError(null);
    getSyntheticControlResult(experimentId)
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
    return <RetryableError message={error} onRetry={fetchData} context="synthetic control analysis" />;
  }

  if (!result) {
    return (
      <div className="rounded-md bg-gray-50 p-4 text-sm text-gray-500">
        No synthetic control analysis available for this experiment.
      </div>
    );
  }

  const postTreatmentEffects = result.effects.filter((e) => e.date >= result.treatmentStartDate.slice(0, 10));
  const finalCumulative = postTreatmentEffects.length > 0
    ? postTreatmentEffects[postTreatmentEffects.length - 1].cumulativeEffect
    : 0;

  return (
    <div className="space-y-6">
      {/* RMSPE diagnostic badge */}
      <RmspeBadge
        preRmspe={result.preRmspe}
        postRmspe={result.postRmspe}
        rmspeRatio={result.rmspeRatio}
        pValue={result.pValue}
      />

      {/* Summary cards */}
      <div className="grid grid-cols-2 gap-4 sm:grid-cols-4">
        <div className="rounded-lg border border-gray-200 bg-white p-3">
          <dt className="text-xs font-medium uppercase text-gray-500">Final Cumulative Effect</dt>
          <dd className={`mt-1 text-xl font-bold ${finalCumulative >= 0 ? 'text-green-700' : 'text-red-700'}`}>
            {finalCumulative >= 0 ? '+' : ''}{finalCumulative.toFixed(4)}
          </dd>
        </div>
        <div className="rounded-lg border border-gray-200 bg-white p-3">
          <dt className="text-xs font-medium uppercase text-gray-500">p-value</dt>
          <dd className={`mt-1 text-xl font-bold ${result.isSignificant ? 'text-green-700' : 'text-gray-700'}`}>
            {formatPValue(result.pValue)}
          </dd>
        </div>
        <div className="rounded-lg border border-gray-200 bg-white p-3">
          <dt className="text-xs font-medium uppercase text-gray-500">Pre-Period RMSPE</dt>
          <dd className="mt-1 text-xl font-bold text-gray-900">{result.preRmspe.toFixed(4)}</dd>
        </div>
        <div className="rounded-lg border border-gray-200 bg-white p-3">
          <dt className="text-xs font-medium uppercase text-gray-500">Donors Used</dt>
          <dd className="mt-1 text-xl font-bold text-gray-900">
            {result.donorWeights.filter((d) => d.weight > 0.001).length}
          </dd>
        </div>
      </div>

      {/* Treated vs synthetic control time series */}
      <TreatedVsSyntheticChart
        timeSeries={result.timeSeries}
        treatmentStartDate={result.treatmentStartDate}
      />

      {/* Pointwise and cumulative effects (side by side on wide screens) */}
      <div className="grid grid-cols-1 gap-4 lg:grid-cols-2">
        <PointwiseEffectsChart effects={result.effects} treatmentStartDate={result.treatmentStartDate} />
        <CumulativeEffectsChart effects={result.effects} treatmentStartDate={result.treatmentStartDate} />
      </div>

      {/* Donor weights */}
      <DonorWeightTable donorWeights={result.donorWeights} />

      {/* Placebo small-multiples */}
      {result.placeboResults.length > 0 && (
        <div>
          <h4 className="mb-1 text-sm font-semibold text-gray-900">
            Placebo Tests ({result.placeboResults.length} donors)
          </h4>
          <p className="mb-3 text-xs text-gray-500">
            Each panel shows the synthetic control effect estimate if that donor were the treated unit.
            The dashed line is the actual treated effect. RMSPE ratio badge colours: green &lt;2×, amber 2–4×, red &gt;4×.
          </p>
          <div className="grid grid-cols-2 gap-3 sm:grid-cols-3 lg:grid-cols-4">
            {result.placeboResults.map((p) => (
              <PlaceboMiniChart
                key={p.donorId}
                placebo={p}
                treatedEffects={result.effects}
              />
            ))}
          </div>
        </div>
      )}
    </div>
  );
}
