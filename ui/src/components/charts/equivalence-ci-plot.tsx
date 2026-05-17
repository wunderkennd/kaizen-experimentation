'use client';

import { memo } from 'react';
import {
  ComposedChart, Scatter, ErrorBar, XAxis, YAxis, CartesianGrid,
  ReferenceLine, ReferenceArea, Tooltip, ResponsiveContainer,
} from 'recharts';
import type { EquivalenceResult } from '@/lib/types';
import { deriveEquivalenceStatus } from '@/components/equivalence-result-badge';

interface EquivalenceCiPlotProps {
  result: EquivalenceResult;
  metricId: string;
}

const STATUS_COLOR: Record<string, string> = {
  EQUIVALENT: '#16a34a',
  INCONCLUSIVE: '#ca8a04',
  NOT_EQUIVALENT: '#dc2626',
};

function EquivalenceCiPlotInner({ result, metricId }: EquivalenceCiPlotProps) {
  const status = deriveEquivalenceStatus(result);
  const color = STATUS_COLOR[status];
  const { delta, ciLower, ciUpper, pointEstimate } = result;

  // Single-row horizontal CI: the point estimate with a (1−2α) error bar,
  // plotted against the shaded [−δ, +δ] equivalence margin.
  const half = Math.max(ciUpper - pointEstimate, pointEstimate - ciLower, 0);
  const chartData = [
    {
      label: metricId,
      estimate: pointEstimate,
      ci: [half, half] as [number, number],
    },
  ];

  // Axis padding so the margin band and the whole CI are always visible.
  const span = Math.max(delta, Math.abs(ciLower), Math.abs(ciUpper));
  const pad = span * 0.2 || 0.1;
  const domain: [number, number] = [-(span + pad), span + pad];

  return (
    <div className="rounded-lg border border-gray-200 bg-white p-4" data-testid="equivalence-ci-plot">
      <div className="mb-3 flex flex-wrap items-center justify-between gap-2">
        <div>
          <h4 className="text-sm font-semibold text-gray-900">
            Equivalence CI — {metricId}
          </h4>
          <p className="mt-0.5 text-xs text-gray-500">
            (1−2α) confidence interval vs. the ±δ equivalence margin (shaded). When
            the interval lies entirely inside the band, the treatment is equivalent.
          </p>
        </div>
      </div>

      <div role="img" aria-label={`Equivalence confidence interval chart for ${metricId}`}>
        <ResponsiveContainer width="100%" height={160}>
          <ComposedChart
            data={chartData}
            layout="vertical"
            margin={{ top: 10, right: 30, bottom: 20, left: 20 }}
          >
            <CartesianGrid strokeDasharray="3 3" stroke="#e5e7eb" />
            <XAxis
              type="number"
              domain={domain}
              tick={{ fontSize: 11 }}
              tickFormatter={(v: number) => v.toFixed(3)}
              label={{ value: 'Treatment − Control', position: 'insideBottom', offset: -10, fontSize: 12 }}
            />
            <YAxis type="category" dataKey="label" tick={{ fontSize: 11 }} width={1} />
            {/* Shaded equivalence margin [−δ, +δ] */}
            <ReferenceArea
              x1={-delta}
              x2={delta}
              fill="#16a34a"
              fillOpacity={0.1}
              label={{ value: '±δ equivalence zone', position: 'insideTop', fontSize: 10, fill: '#15803d' }}
            />
            <ReferenceLine x={0} stroke="#6b7280" strokeDasharray="4 4" />
            <ReferenceLine x={-delta} stroke="#16a34a" strokeDasharray="2 2" />
            <ReferenceLine x={delta} stroke="#16a34a" strokeDasharray="2 2" />
            <Tooltip
              formatter={(value: number) => [value.toFixed(4), 'Effect']}
              labelFormatter={() => metricId}
            />
            <Scatter dataKey="estimate" fill={color} isAnimationActive={false}>
              <ErrorBar dataKey="ci" direction="x" width={6} strokeWidth={2} stroke={color} />
            </Scatter>
          </ComposedChart>
        </ResponsiveContainer>
      </div>

      <dl className="mt-3 grid grid-cols-2 gap-x-6 gap-y-1 text-xs text-gray-600 sm:grid-cols-4">
        <div>
          <dt className="text-gray-400">Point estimate</dt>
          <dd className="font-medium text-gray-900" data-testid="equiv-point-estimate">
            {pointEstimate.toFixed(4)}
          </dd>
        </div>
        <div>
          <dt className="text-gray-400">(1−2α) CI</dt>
          <dd className="font-medium text-gray-900" data-testid="equiv-ci">
            [{ciLower.toFixed(4)}, {ciUpper.toFixed(4)}]
          </dd>
        </div>
        <div>
          <dt className="text-gray-400">Margin δ</dt>
          <dd className="font-medium text-gray-900" data-testid="equiv-delta">
            ±{delta.toFixed(4)}
          </dd>
        </div>
        <div>
          <dt className="text-gray-400">TOST p-value</dt>
          <dd className="font-medium text-gray-900" data-testid="equiv-p-tost">
            {result.pTost.toFixed(4)}
          </dd>
        </div>
      </dl>
    </div>
  );
}

export const EquivalenceCiPlot = memo(EquivalenceCiPlotInner);
