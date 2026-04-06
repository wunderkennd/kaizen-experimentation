'use client';

import { memo } from 'react';
import {
  ScatterChart,
  Scatter,
  XAxis,
  YAxis,
  Tooltip,
  ResponsiveContainer,
  CartesianGrid,
  ZAxis,
  Line as ReferenceLine,
} from 'recharts';
import type { ParetoPoint } from '@/lib/types';

interface ParetoFrontierPlotProps {
  points: ParetoPoint[];
  frontierIds: string[];
}

/**
 * Scatter plot showing multi-objective experiment outcomes.
 * Pareto-optimal points are highlighted; the efficient frontier is drawn as a connected line.
 */
export const ParetoFrontierPlot = memo(function ParetoFrontierPlot({
  points,
  frontierIds,
}: ParetoFrontierPlotProps) {
  if (points.length === 0) {
    return (
      <div className="rounded-lg border border-gray-200 bg-white p-6" data-testid="pareto-frontier-plot">
        <h3 className="text-sm font-semibold text-gray-900">Pareto Frontier</h3>
        <p className="mt-8 text-center text-sm text-gray-500">No multi-objective data available.</p>
      </div>
    );
  }

  const xLabel = points[0]?.objectiveXLabel ?? 'Objective X';
  const yLabel = points[0]?.objectiveYLabel ?? 'Objective Y';

  const frontierSet = new Set(frontierIds);
  const paretoPoints = points.filter((p) => frontierSet.has(p.experimentId));
  const nonParetoPoints = points.filter((p) => !frontierSet.has(p.experimentId));

  // Sort frontier points by objectiveX for the connecting line
  const sortedFrontier = [...paretoPoints].sort((a, b) => a.objectiveX - b.objectiveX);

  return (
    <div className="rounded-lg border border-gray-200 bg-white p-4" data-testid="pareto-frontier-plot">
      <h3 className="mb-3 text-sm font-semibold text-gray-900">Pareto Frontier</h3>
      <p className="mb-2 text-xs text-gray-500">
        Multi-objective trade-offs — frontier points represent optimal configurations
      </p>
      <div
        role="img"
        aria-label={`Pareto frontier with ${paretoPoints.length} optimal and ${nonParetoPoints.length} dominated points`}
      >
        <ResponsiveContainer width="100%" height={320}>
          <ScatterChart margin={{ top: 10, right: 20, bottom: 30, left: 10 }}>
            <CartesianGrid strokeDasharray="3 3" stroke="#e5e7eb" />
            <XAxis
              type="number"
              dataKey="objectiveX"
              name={xLabel}
              tick={{ fontSize: 11 }}
              label={{ value: xLabel, position: 'insideBottom', offset: -10, fontSize: 12 }}
            />
            <YAxis
              type="number"
              dataKey="objectiveY"
              name={yLabel}
              tick={{ fontSize: 11 }}
              label={{ value: yLabel, angle: -90, position: 'insideLeft', offset: 10, fontSize: 12 }}
            />
            <ZAxis range={[60, 60]} />
            <Tooltip
              content={({ payload }) => {
                if (!payload?.length) return null;
                const p = payload[0]?.payload as ParetoPoint | undefined;
                if (!p) return null;
                return (
                  <div className="rounded border border-gray-200 bg-white p-2 text-xs shadow-sm">
                    <p className="font-semibold">{p.experimentName}</p>
                    <p>{xLabel}: {p.objectiveX.toFixed(4)}</p>
                    <p>{yLabel}: {p.objectiveY.toFixed(4)}</p>
                    {p.isPareto && (
                      <p className="mt-1 font-medium text-indigo-600">Pareto optimal</p>
                    )}
                  </div>
                );
              }}
            />
            {/* Dominated points in gray */}
            <Scatter
              name="Dominated"
              data={nonParetoPoints}
              fill="#9ca3af"
              isAnimationActive={false}
            />
            {/* Frontier points in indigo */}
            <Scatter
              name="Pareto Optimal"
              data={paretoPoints}
              fill="#4f46e5"
              isAnimationActive={false}
            />
            {/* Frontier connecting line — rendered via a second Scatter with line=true */}
            {sortedFrontier.length > 1 && (
              <Scatter
                data={sortedFrontier}
                fill="none"
                line={{ stroke: '#4f46e5', strokeWidth: 2, strokeDasharray: '4 4' }}
                isAnimationActive={false}
                legendType="none"
              />
            )}
          </ScatterChart>
        </ResponsiveContainer>
      </div>

      {/* Legend */}
      <div className="mt-3 flex gap-4 text-xs text-gray-600">
        <span className="flex items-center gap-1.5">
          <span className="inline-block h-3 w-3 rounded-full bg-indigo-600" />
          Pareto optimal ({paretoPoints.length})
        </span>
        <span className="flex items-center gap-1.5">
          <span className="inline-block h-3 w-3 rounded-full bg-gray-400" />
          Dominated ({nonParetoPoints.length})
        </span>
      </div>
    </div>
  );
});
