'use client';

import { useEffect, useState, useCallback, useRef } from 'react';
import type { Experiment, AnalysisResult, GuardrailStatusResult } from '@/lib/types';
import { listExperiments, getAnalysisResult, getGuardrailStatus } from '@/lib/api';
import { MonitoringSummaryCards } from '@/components/monitoring-summary-cards';
import { MonitoringHealthTable } from '@/components/monitoring-health-table';
import { MonitoringBreachList } from '@/components/monitoring-breach-list';
import { RetryableError } from '@/components/retryable-error';

const AUTO_REFRESH_INTERVAL_MS = 30_000;

export default function MonitoringPage() {
  const [experiments, setExperiments] = useState<Experiment[]>([]);
  const [analysisResults, setAnalysisResults] = useState<Record<string, AnalysisResult>>({});
  const [guardrailStatuses, setGuardrailStatuses] = useState<Record<string, GuardrailStatusResult>>({});
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [autoRefresh, setAutoRefresh] = useState(false);
  const [lastUpdated, setLastUpdated] = useState<Date | null>(null);
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null);

  const fetchData = useCallback(async () => {
    setLoading(true);
    setError(null);
    clearApiCache();
    setError(null);

    try {
      const listResult = await listExperiments();
      const allExperiments = listResult.experiments;
      setExperiments(allExperiments);

      const runningExperiments = allExperiments.filter((e) => e.state === 'RUNNING');

      // Fetch analysis results and guardrail statuses in parallel for all running experiments
      const [analysisEntries, guardrailEntries] = await Promise.all([
        Promise.all(
          runningExperiments.map(async (exp) => {
            try {
              const result = await getAnalysisResult(exp.experimentId);
              return [exp.experimentId, result] as const;
            } catch {
              return null;
            }
          }),
        ),
        Promise.all(
          runningExperiments.map(async (exp) => {
            try {
              const result = await getGuardrailStatus(exp.experimentId);
              return [exp.experimentId, result] as const;
            } catch {
              return null;
            }
          }),
        ),
      ]);

      const analysisMap: Record<string, AnalysisResult> = {};
      for (const entry of analysisEntries) {
        if (entry) {
          analysisMap[entry[0]] = entry[1];
        }
      }

      const guardrailMap: Record<string, GuardrailStatusResult> = {};
      for (const entry of guardrailEntries) {
        if (entry) {
          guardrailMap[entry[0]] = entry[1];
        }
      }

      setAnalysisResults(analysisMap);
      setGuardrailStatuses(guardrailMap);
      setLastUpdated(new Date());
    } catch (err) {
      setError(err instanceof Error ? err.message : 'An unknown error occurred');
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    fetchData();
  }, [fetchData]);

  useEffect(() => {
    if (autoRefresh) {
      intervalRef.current = setInterval(() => {
        fetchData();
      }, AUTO_REFRESH_INTERVAL_MS);
    } else if (intervalRef.current) {
      clearInterval(intervalRef.current);
      intervalRef.current = null;
    }
    return () => {
      if (intervalRef.current) {
        clearInterval(intervalRef.current);
      }
    };
  }, [autoRefresh, fetchData]);

  if (loading && experiments.length === 0) {
    return (
      <div className="flex items-center justify-center py-12" role="status" aria-label="Loading">
        <div className="h-8 w-8 animate-spin rounded-full border-4 border-gray-300 border-t-indigo-600" />
        <span className="sr-only">Loading</span>
      </div>
    );
  }

  if (error && experiments.length === 0) {
    return <RetryableError message={error} onRetry={fetchData} context="monitoring data" />;
  }

  return (
    <div>
      <div className="mb-6 flex items-center justify-between">
        <h1 className="text-2xl font-bold text-gray-900">Monitoring</h1>
        <div className="flex items-center gap-4">
          {lastUpdated && (
            <span className="text-sm text-gray-500" data-testid="last-updated">
              Last updated: {lastUpdated.toLocaleTimeString()}
            </span>
          )}
          <label className="flex items-center gap-2 text-sm text-gray-700" data-testid="auto-refresh-label">
            <input
              type="checkbox"
              checked={autoRefresh}
              onChange={(e) => setAutoRefresh(e.target.checked)}
              className="h-4 w-4 rounded border-gray-300 text-indigo-600 focus:ring-indigo-500"
              data-testid="auto-refresh-toggle"
            />
            Auto-refresh (30s)
          </label>
        </div>
      </div>

      <section className="mb-8" aria-labelledby="summary-heading">
        <h2 id="summary-heading" className="mb-4 text-lg font-semibold text-gray-800">
          Active Experiments Summary
        </h2>
        <MonitoringSummaryCards experiments={experiments} />
      </section>

      <section className="mb-8" aria-labelledby="health-heading">
        <h2 id="health-heading" className="mb-4 text-lg font-semibold text-gray-800">
          Running Experiments Health
        </h2>
        <MonitoringHealthTable
          experiments={experiments}
          analysisResults={analysisResults}
          guardrailStatuses={guardrailStatuses}
        />
      </section>

      <section aria-labelledby="breaches-heading">
        <h2 id="breaches-heading" className="mb-4 text-lg font-semibold text-gray-800">
          Recent Guardrail Breaches
        </h2>
        <MonitoringBreachList
          experiments={experiments}
          guardrailStatuses={guardrailStatuses}
        />
      </section>
    </div>
  );
}
