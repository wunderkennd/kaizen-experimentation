'use client';

import { useEffect, useState, useMemo } from 'react';
import {
  BarChart, Bar, XAxis, YAxis, Tooltip, ResponsiveContainer, Cell,
} from 'recharts';
import type { Layer, LayerAllocation } from '@/lib/types';
import { getLayer, getLayerAllocations } from '@/lib/api';

interface LayerAllocationChartProps {
  layerId: string;
  currentExperimentId: string;
}

/** Maps experiment IDs to display colors. Current experiment = indigo, others cycle through palette. */
const PALETTE = ['#0891b2', '#059669', '#d97706', '#dc2626'];
const CURRENT_COLOR = '#4f46e5';
const UNALLOCATED_COLOR = '#e5e7eb';

interface Segment {
  name: string;
  width: number;
  startBucket: number;
  endBucket: number;
  color: string;
  isCurrent: boolean;
  isUnallocated: boolean;
  experimentId?: string;
}

function buildSegments(
  allocations: LayerAllocation[],
  totalBuckets: number,
  currentExperimentId: string,
  experiments: Map<string, string>,
): Segment[] {
  // Sort allocations by startBucket
  const sorted = [...allocations].sort((a, b) => a.startBucket - b.startBucket);

  const segments: Segment[] = [];
  let cursor = 0;
  let otherColorIdx = 0;

  for (const alloc of sorted) {
    // Gap before this allocation
    if (alloc.startBucket > cursor) {
      segments.push({
        name: 'Unallocated',
        width: alloc.startBucket - cursor,
        startBucket: cursor,
        endBucket: alloc.startBucket - 1,
        color: UNALLOCATED_COLOR,
        isCurrent: false,
        isUnallocated: true,
      });
    }

    const isCurrent = alloc.experimentId === currentExperimentId;
    const color = isCurrent ? CURRENT_COLOR : PALETTE[otherColorIdx++ % PALETTE.length];
    const expName = experiments.get(alloc.experimentId) || alloc.experimentId.slice(0, 8);

    segments.push({
      name: expName,
      width: alloc.endBucket - alloc.startBucket + 1,
      startBucket: alloc.startBucket,
      endBucket: alloc.endBucket,
      color,
      isCurrent,
      isUnallocated: false,
      experimentId: alloc.experimentId,
    });

    cursor = alloc.endBucket + 1;
  }

  // Trailing unallocated
  if (cursor < totalBuckets) {
    segments.push({
      name: 'Unallocated',
      width: totalBuckets - cursor,
      startBucket: cursor,
      endBucket: totalBuckets - 1,
      color: UNALLOCATED_COLOR,
      isCurrent: false,
      isUnallocated: true,
    });
  }

  return segments;
}

export function LayerAllocationChart({ layerId, currentExperimentId }: LayerAllocationChartProps) {
  const [layer, setLayer] = useState<Layer | null>(null);
  const [allocations, setAllocations] = useState<LayerAllocation[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;

    Promise.all([getLayer(layerId), getLayerAllocations(layerId)])
      .then(([l, a]) => {
        if (cancelled) return;
        setLayer(l);
        setAllocations(a);
      })
      .catch((err) => {
        if (cancelled) return;
        setError(err.message);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });

    return () => { cancelled = true; };
  }, [layerId]);

  // Build a name lookup for experiments referenced in allocations
  const experimentNames = useMemo(() => {
    const map = new Map<string, string>();
    // We only know the current experiment's ID — others will show truncated IDs
    // In production this would be resolved by a batch lookup
    return map;
  }, []);

  const segments = useMemo(
    () => layer ? buildSegments(allocations, layer.totalBuckets, currentExperimentId, experimentNames) : [],
    [layer, allocations, currentExperimentId, experimentNames],
  );

  if (loading) {
    return (
      <div className="flex items-center justify-center py-6" role="status" aria-label="Loading layer allocation">
        <div className="h-5 w-5 animate-spin rounded-full border-4 border-gray-300 border-t-indigo-600" />
        <span className="sr-only">Loading</span>
      </div>
    );
  }

  if (error) {
    return (
      <div className="rounded-md bg-red-50 p-3 text-sm text-red-700">
        Failed to load layer allocation: {error}
      </div>
    );
  }

  if (!layer) return null;

  // Empty state for layers with no allocations (DRAFT experiments)
  if (allocations.length === 0) {
    return (
      <div className="rounded-md bg-gray-50 p-4 text-sm text-gray-500" data-testid="layer-empty-state">
        No bucket allocations yet. Allocations are created when the experiment starts.
      </div>
    );
  }

  // Transform segments into stacked bar data — single row with each segment as a field
  const chartData: Record<string, number> = {};
  segments.forEach((seg, i) => {
    chartData[`seg_${i}`] = seg.width;
  });

  return (
    <div className="space-y-3">
      {/* Layer name */}
      <div className="flex items-center justify-between">
        <span className="text-sm font-medium text-gray-700">
          Layer: <span className="font-semibold">{layer.name}</span>
        </span>
        <span className="text-xs text-gray-500">{layer.totalBuckets.toLocaleString()} total buckets</span>
      </div>

      {/* Stacked horizontal bar */}
      <div
        style={{ width: '100%', height: 56 }}
        role="img"
        aria-label={`Bucket allocation for ${layer.name} layer: ${allocations.length} experiment(s) allocated`}
      >
        <ResponsiveContainer>
          <BarChart
            layout="vertical"
            data={[chartData]}
            margin={{ top: 0, right: 0, bottom: 0, left: 0 }}
            barCategoryGap={0}
          >
            <XAxis type="number" hide domain={[0, layer.totalBuckets]} />
            <YAxis type="category" hide dataKey={() => 'allocation'} />
            <Tooltip
              cursor={false}
              content={({ active, payload }) => {
                if (!active || !payload?.length) return null;
                const entry = payload[0];
                const segIdx = parseInt(String(entry.dataKey).replace('seg_', ''), 10);
                const seg = segments[segIdx];
                if (!seg) return null;
                return (
                  <div className="rounded border border-gray-200 bg-white px-3 py-2 text-xs shadow-lg">
                    <p className="font-semibold">{seg.name}</p>
                    <p className="text-gray-600">Buckets {seg.startBucket.toLocaleString()} - {seg.endBucket.toLocaleString()}</p>
                    <p className="text-gray-600">{((seg.width / layer.totalBuckets) * 100).toFixed(1)}% of traffic</p>
                  </div>
                );
              }}
            />
            {segments.map((seg, i) => (
              <Bar
                key={`seg_${i}`}
                dataKey={`seg_${i}`}
                stackId="allocation"
                fill={seg.color}
                isAnimationActive={false}
              >
                <Cell fill={seg.color} />
              </Bar>
            ))}
          </BarChart>
        </ResponsiveContainer>
      </div>

      {/* Legend table */}
      <table className="min-w-full text-sm" data-testid="layer-legend-table">
        <thead>
          <tr className="border-b border-gray-200">
            <th className="pb-1 text-left text-xs font-medium uppercase text-gray-500">Experiment</th>
            <th className="pb-1 text-right text-xs font-medium uppercase text-gray-500">Bucket Range</th>
            <th className="pb-1 text-right text-xs font-medium uppercase text-gray-500">Traffic %</th>
          </tr>
        </thead>
        <tbody>
          {segments.filter((s) => !s.isUnallocated).map((seg, i) => (
            <tr key={i} className="border-b border-gray-100">
              <td className="py-1.5 text-gray-900">
                <span className="mr-2 inline-block h-3 w-3 rounded" style={{ backgroundColor: seg.color }} />
                {seg.name}
                {seg.isCurrent && (
                  <span className="ml-1.5 inline-flex items-center rounded bg-indigo-50 px-1.5 py-0.5 text-xs font-medium text-indigo-700">
                    current
                  </span>
                )}
              </td>
              <td className="py-1.5 text-right font-mono text-gray-600">
                {seg.startBucket.toLocaleString()} - {seg.endBucket.toLocaleString()}
              </td>
              <td className="py-1.5 text-right font-mono text-gray-600">
                {((seg.width / (layer?.totalBuckets || 10000)) * 100).toFixed(1)}%
              </td>
            </tr>
          ))}
          {/* Unallocated summary */}
          {segments.some((s) => s.isUnallocated) && (
            <tr className="border-b border-gray-100">
              <td className="py-1.5 text-gray-400">
                <span className="mr-2 inline-block h-3 w-3 rounded" style={{ backgroundColor: UNALLOCATED_COLOR }} />
                Unallocated
              </td>
              <td className="py-1.5 text-right font-mono text-gray-400">
                {segments.filter((s) => s.isUnallocated).map((s) => `${s.startBucket.toLocaleString()}-${s.endBucket.toLocaleString()}`).join(', ')}
              </td>
              <td className="py-1.5 text-right font-mono text-gray-400">
                {((segments.filter((s) => s.isUnallocated).reduce((sum, s) => sum + s.width, 0) / (layer?.totalBuckets || 10000)) * 100).toFixed(1)}%
              </td>
            </tr>
          )}
        </tbody>
      </table>
    </div>
  );
}
