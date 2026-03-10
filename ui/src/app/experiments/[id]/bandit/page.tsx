'use client';

import { useEffect, useState } from 'react';
import { useParams } from 'next/navigation';
import Link from 'next/link';
import {
  BarChart,
  Bar,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  ResponsiveContainer,
  Cell,
} from 'recharts';
import type { Experiment, BanditDashboardResult } from '@/lib/types';
import { getExperiment, getBanditDashboard } from '@/lib/api';
import { formatDate } from '@/lib/utils';

const ARM_COLORS = ['#4f46e5', '#0891b2', '#059669', '#d97706', '#dc2626', '#7c3aed'];

export default function BanditDashboardPage() {
  const params = useParams<{ id: string }>();
  const [experiment, setExperiment] = useState<Experiment | null>(null);
  const [dashboard, setDashboard] = useState<BanditDashboardResult | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!params.id) return;

    Promise.all([getExperiment(params.id), getBanditDashboard(params.id)])
      .then(([exp, bd]) => {
        setExperiment(exp);
        setDashboard(bd);
      })
      .catch((err) => setError(err.message))
      .finally(() => setLoading(false));
  }, [params.id]);

  if (loading) {
    return (
      <div className="flex items-center justify-center py-12" role="status" aria-label="Loading">
        <div className="h-8 w-8 animate-spin rounded-full border-4 border-gray-300 border-t-indigo-600" />
        <span className="sr-only">Loading</span>
      </div>
    );
  }

  if (error || !experiment || !dashboard) {
    return (
      <div>
        <nav className="mb-4 text-sm text-gray-500">
          <Link href="/" className="hover:text-indigo-600">Experiments</Link>
          <span className="mx-2">/</span>
          <Link href={`/experiments/${params.id}`} className="hover:text-indigo-600">Detail</Link>
          <span className="mx-2">/</span>
          <span className="text-gray-900">Bandit</span>
        </nav>
        <div className="rounded-md bg-red-50 p-4">
          <p className="text-sm text-red-700">
            {error || 'No bandit dashboard available for this experiment.'}
          </p>
        </div>
      </div>
    );
  }

  const allocationData = dashboard.arms.map((arm) => ({
    name: arm.name,
    probability: arm.assignmentProbability,
  }));

  const rewardRateData = dashboard.arms.map((arm) => ({
    name: arm.name,
    rewardRate: arm.rewardRate,
  }));

  // Build cumulative reward time series from rewardHistory
  const uniqueTimestamps = [...new Set(dashboard.rewardHistory.map((p) => p.timestamp))].sort();
  const rewardCurveData = uniqueTimestamps.map((ts) => {
    const point: Record<string, unknown> = { date: formatDate(ts) };
    for (const arm of dashboard.arms) {
      const entry = dashboard.rewardHistory.find((p) => p.timestamp === ts && p.armId === arm.name);
      if (entry) {
        point[arm.name] = entry.cumulativeSelections > 0
          ? entry.cumulativeReward / entry.cumulativeSelections
          : 0;
      }
    }
    return point;
  });

  const algorithmLabel = dashboard.algorithm.replace(/_/g, ' ');

  return (
    <div>
      {/* Breadcrumb */}
      <nav className="mb-4 text-sm text-gray-500">
        <Link href="/" className="hover:text-indigo-600">Experiments</Link>
        <span className="mx-2">/</span>
        <Link href={`/experiments/${params.id}`} className="hover:text-indigo-600">Detail</Link>
        <span className="mx-2">/</span>
        <span className="text-gray-900">Bandit</span>
      </nav>

      <h1 className="mb-6 text-2xl font-bold text-gray-900">Bandit Dashboard</h1>

      {/* Summary bar */}
      <div className="mb-6 flex items-center gap-6 rounded-lg border border-gray-200 bg-white px-4 py-3">
        <div>
          <span className="text-xs font-medium uppercase text-gray-500">Experiment</span>
          <p className="text-sm font-medium text-gray-900">{experiment.name}</p>
        </div>
        <div>
          <span className="text-xs font-medium uppercase text-gray-500">Algorithm</span>
          <p className="text-sm text-gray-900">{algorithmLabel}</p>
        </div>
        <div>
          <span className="text-xs font-medium uppercase text-gray-500">Rewards Processed</span>
          <p className="text-sm text-gray-900">{dashboard.totalRewardsProcessed.toLocaleString()}</p>
        </div>
        <div>
          <span className="text-xs font-medium uppercase text-gray-500">Last Snapshot</span>
          <p className="text-sm text-gray-900">{formatDate(dashboard.snapshotAt)}</p>
        </div>
        <div>
          <span className="text-xs font-medium uppercase text-gray-500">Status</span>
          <p className="text-sm">
            {dashboard.isWarmup ? (
              <span className="inline-flex items-center rounded-full bg-yellow-100 px-2.5 py-0.5 text-xs font-medium text-yellow-800">
                Warmup ({dashboard.warmupObservations} obs)
              </span>
            ) : (
              <span className="inline-flex items-center rounded-full bg-green-100 px-2.5 py-0.5 text-xs font-medium text-green-800">
                Active
              </span>
            )}
          </p>
        </div>
      </div>

      {/* Arm allocation chart */}
      <section className="mb-6">
        <h2 className="mb-3 text-lg font-semibold text-gray-900">Arm Allocation</h2>
        <div className="rounded-lg border border-gray-200 bg-white p-4">
          <div role="img" aria-label="Arm allocation probabilities">
          <ResponsiveContainer width="100%" height={220}>
            <BarChart data={allocationData} margin={{ left: 20, right: 20, top: 10, bottom: 10 }}>
              <CartesianGrid strokeDasharray="3 3" vertical={false} />
              <XAxis dataKey="name" tick={{ fontSize: 12 }} />
              <YAxis domain={[0, 1]} tickFormatter={(v: number) => `${(v * 100).toFixed(0)}%`} />
              <Tooltip formatter={(v: number) => `${(v * 100).toFixed(1)}%`} />
              <Bar dataKey="probability" isAnimationActive={false}>
                {allocationData.map((_, i) => (
                  <Cell key={`alloc-${i}`} fill={ARM_COLORS[i % ARM_COLORS.length]} />
                ))}
              </Bar>
            </BarChart>
          </ResponsiveContainer>
          </div>
          <p className="mt-2 text-xs text-gray-500">
            Min exploration floor: {(dashboard.minExplorationFraction * 100).toFixed(0)}%
          </p>
        </div>
      </section>

      {/* Reward rates chart */}
      <section className="mb-6">
        <h2 className="mb-3 text-lg font-semibold text-gray-900">Reward Rates</h2>
        <div className="rounded-lg border border-gray-200 bg-white p-4">
          <div role="img" aria-label="Reward rates per arm">
          <ResponsiveContainer width="100%" height={220}>
            <BarChart data={rewardRateData} margin={{ left: 20, right: 20, top: 10, bottom: 10 }}>
              <CartesianGrid strokeDasharray="3 3" vertical={false} />
              <XAxis dataKey="name" tick={{ fontSize: 12 }} />
              <YAxis domain={[0, 'auto']} tickFormatter={(v: number) => `${(v * 100).toFixed(0)}%`} />
              <Tooltip formatter={(v: number) => `${(v * 100).toFixed(1)}%`} />
              <Bar dataKey="rewardRate" isAnimationActive={false}>
                {rewardRateData.map((_, i) => (
                  <Cell key={`rr-${i}`} fill={ARM_COLORS[i % ARM_COLORS.length]} />
                ))}
              </Bar>
            </BarChart>
          </ResponsiveContainer>
          </div>
        </div>
      </section>

      {/* Reward curve over time */}
      {rewardCurveData.length > 0 && (
        <section className="mb-6">
          <h2 className="mb-3 text-lg font-semibold text-gray-900">Reward Rate Over Time</h2>
          <div className="rounded-lg border border-gray-200 bg-white p-4">
            <div role="img" aria-label="Reward rate history">
            <ResponsiveContainer width="100%" height={250}>
              <BarChart data={rewardCurveData} margin={{ left: 20, right: 20, top: 10, bottom: 10 }}>
                <CartesianGrid strokeDasharray="3 3" vertical={false} />
                <XAxis dataKey="date" tick={{ fontSize: 11 }} />
                <YAxis tickFormatter={(v: number) => `${(v * 100).toFixed(0)}%`} />
                <Tooltip formatter={(v: number) => `${(v * 100).toFixed(1)}%`} />
                {dashboard.arms.map((arm, i) => (
                  <Bar key={arm.name} dataKey={arm.name} fill={ARM_COLORS[i % ARM_COLORS.length]} isAnimationActive={false} />
                ))}
              </BarChart>
            </ResponsiveContainer>
            </div>
          </div>
        </section>
      )}

      {/* Arm stats table */}
      <section className="mb-6">
        <h2 className="mb-3 text-lg font-semibold text-gray-900">Arm Statistics</h2>
        <div className="overflow-hidden rounded-lg border border-gray-200 bg-white">
          <table className="min-w-full divide-y divide-gray-200">
            <thead className="bg-gray-50">
              <tr>
                <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">Arm</th>
                <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">Selections</th>
                <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">Rewards</th>
                <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">Reward Rate</th>
                <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">Allocation</th>
                {dashboard.algorithm === 'THOMPSON_SAMPLING' && (
                  <>
                    <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">Alpha</th>
                    <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">Beta</th>
                  </>
                )}
              </tr>
            </thead>
            <tbody className="divide-y divide-gray-200">
              {dashboard.arms.map((arm, i) => (
                <tr key={arm.armId}>
                  <td className="whitespace-nowrap px-4 py-3 text-sm font-medium text-gray-900">
                    <span className="mr-2 inline-block h-2.5 w-2.5 rounded-full" style={{ backgroundColor: ARM_COLORS[i % ARM_COLORS.length] }} aria-hidden="true" />
                    {arm.name}
                  </td>
                  <td className="whitespace-nowrap px-4 py-3 text-sm text-gray-600">{arm.selectionCount.toLocaleString()}</td>
                  <td className="whitespace-nowrap px-4 py-3 text-sm text-gray-600">{arm.rewardCount.toLocaleString()}</td>
                  <td className="whitespace-nowrap px-4 py-3 text-sm text-gray-600">{(arm.rewardRate * 100).toFixed(1)}%</td>
                  <td className="whitespace-nowrap px-4 py-3 text-sm text-gray-600">{(arm.assignmentProbability * 100).toFixed(1)}%</td>
                  {dashboard.algorithm === 'THOMPSON_SAMPLING' && (
                    <>
                      <td className="whitespace-nowrap px-4 py-3 text-sm text-gray-600">{arm.alpha?.toFixed(0)}</td>
                      <td className="whitespace-nowrap px-4 py-3 text-sm text-gray-600">{arm.beta?.toFixed(0)}</td>
                    </>
                  )}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </section>
    </div>
  );
}
