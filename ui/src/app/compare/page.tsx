'use client';

import { useEffect, useState, useCallback } from 'react';
import Link from 'next/link';
import dynamic from 'next/dynamic';
import type { Experiment, AnalysisResult } from '@/lib/types';
import { listExperiments, getAnalysisResult } from '@/lib/api';
import { RetryableError } from '@/components/retryable-error';
import { ExperimentSelector } from '@/components/experiment-selector';
import { ComparisonTable } from '@/components/comparison-table';

// Dynamic import for chart — recharts only loads when needed
const ComparisonChart = dynamic(
  () => import('@/components/comparison-chart').then((m) => ({ default: m.ComparisonChart })),
  { ssr: false },
);

interface ComparisonEntry {
  experiment: Experiment;
  analysisResult: AnalysisResult;
}

export default function ComparePage() {
  const [experiments, setExperiments] = useState<Experiment[]>([]);
  const [selectedIds, setSelectedIds] = useState<string[]>([]);
  const [entries, setEntries] = useState<ComparisonEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [loadingResults, setLoadingResults] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [resultsError, setResultsError] = useState<string | null>(null);

  const fetchExperiments = useCallback(() => {
    setLoading(true);
    setError(null);
    listExperiments()
      .then((res) => setExperiments(res.experiments))
      .catch((err) => setError(err.message))
      .finally(() => setLoading(false));
  }, []);

  useEffect(() => {
    fetchExperiments();
  }, [fetchExperiments]);

  // Fetch analysis results when selectedIds change
  useEffect(() => {
    if (selectedIds.length === 0) {
      setEntries([]);
      return;
    }

    setLoadingResults(true);
    setResultsError(null);

    const fetchResults = async () => {
      try {
        const results: ComparisonEntry[] = [];
        for (const id of selectedIds) {
          const experiment = experiments.find((e) => e.experimentId === id);
          if (!experiment) continue;
          const analysisResult = await getAnalysisResult(id);
          results.push({ experiment, analysisResult });
        }
        setEntries(results);
      } catch (err) {
        setResultsError(err instanceof Error ? err.message : 'Failed to fetch analysis results');
      } finally {
        setLoadingResults(false);
      }
    };

    fetchResults();
  }, [selectedIds, experiments]);

  const handleSelect = useCallback((id: string) => {
    setSelectedIds((prev) => [...prev, id]);
  }, []);

  const handleRemove = useCallback((id: string) => {
    setSelectedIds((prev) => prev.filter((sid) => sid !== id));
  }, []);

  if (loading) {
    return (
      <div className="flex items-center justify-center py-12" role="status" aria-label="Loading">
        <div className="h-8 w-8 animate-spin rounded-full border-4 border-gray-300 border-t-indigo-600" />
        <span className="sr-only">Loading</span>
      </div>
    );
  }

  if (error) {
    return (
      <div>
        <nav className="mb-4 text-sm text-gray-500">
          <Link href="/" className="hover:text-indigo-600">Experiments</Link>
          <span className="mx-2">/</span>
          <span className="text-gray-900">Compare</span>
        </nav>
        <RetryableError message={error} onRetry={fetchExperiments} context="experiments" />
      </div>
    );
  }

  return (
    <div>
      {/* Breadcrumb */}
      <nav className="mb-4 text-sm text-gray-500">
        <Link href="/" className="hover:text-indigo-600">Experiments</Link>
        <span className="mx-2">/</span>
        <span className="text-gray-900">Compare</span>
      </nav>

      <h1 className="mb-6 text-2xl font-bold text-gray-900">Experiment Comparison</h1>

      {/* Experiment selector */}
      <ExperimentSelector
        experiments={experiments}
        selectedIds={selectedIds}
        onSelect={handleSelect}
        onRemove={handleRemove}
      />

      {/* Empty state */}
      {selectedIds.length === 0 && (
        <div className="rounded-lg border-2 border-dashed border-gray-300 p-12 text-center" data-testid="empty-state">
          <p className="text-lg font-medium text-gray-500">No experiments selected</p>
          <p className="mt-1 text-sm text-gray-400">
            Select 2 or more experiments above to compare their analysis results side by side.
          </p>
        </div>
      )}

      {/* Loading results */}
      {loadingResults && selectedIds.length > 0 && (
        <div className="flex items-center justify-center py-8" role="status" aria-label="Loading results">
          <div className="h-6 w-6 animate-spin rounded-full border-4 border-gray-300 border-t-indigo-600" />
          <span className="ml-3 text-sm text-gray-500">Loading analysis results...</span>
        </div>
      )}

      {/* Results error */}
      {resultsError && (
        <RetryableError
          message={resultsError}
          onRetry={() => {
            // Trigger re-fetch by resetting and re-setting IDs
            const ids = [...selectedIds];
            setSelectedIds([]);
            setTimeout(() => setSelectedIds(ids), 0);
          }}
          context="analysis results"
        />
      )}

      {/* Comparison content */}
      {!loadingResults && !resultsError && entries.length >= 2 && (
        <>
          <ComparisonChart entries={entries} />
          <ComparisonTable entries={entries} />
        </>
      )}

      {/* Insufficient selections message */}
      {!loadingResults && !resultsError && selectedIds.length === 1 && entries.length === 1 && (
        <div className="rounded-lg border border-yellow-200 bg-yellow-50 p-6 text-center" data-testid="select-more">
          <p className="text-sm font-medium text-yellow-800">
            Select at least one more experiment to begin comparison.
          </p>
        </div>
      )}
    </div>
  );
}
