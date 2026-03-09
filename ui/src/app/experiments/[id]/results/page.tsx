'use client';

import { useEffect, useState } from 'react';
import { useParams } from 'next/navigation';
import Link from 'next/link';
import type { Experiment, AnalysisResult } from '@/lib/types';
import { getExperiment, getAnalysisResult } from '@/lib/api';
import { SrmBanner } from '@/components/srm-banner';
import { ResultsSummary } from '@/components/results-summary';
import { CupedToggle } from '@/components/cuped-toggle';
import { TreatmentEffectsTable } from '@/components/treatment-effects-table';
import { ForestPlot } from '@/components/charts/forest-plot';
import { SequentialBoundaryPlot } from '@/components/charts/sequential-boundary-plot';

export default function ResultsPage() {
  const params = useParams<{ id: string }>();
  const [experiment, setExperiment] = useState<Experiment | null>(null);
  const [analysisResult, setAnalysisResult] = useState<AnalysisResult | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [showCuped, setShowCuped] = useState(false);

  useEffect(() => {
    if (!params.id) return;

    Promise.all([getExperiment(params.id), getAnalysisResult(params.id)])
      .then(([exp, analysis]) => {
        setExperiment(exp);
        setAnalysisResult(analysis);
      })
      .catch((err) => setError(err.message))
      .finally(() => setLoading(false));
  }, [params.id]);

  if (loading) {
    return (
      <div className="flex items-center justify-center py-12">
        <div className="h-8 w-8 animate-spin rounded-full border-4 border-gray-300 border-t-indigo-600" />
      </div>
    );
  }

  if (error || !experiment || !analysisResult) {
    return (
      <div>
        <nav className="mb-4 text-sm text-gray-500">
          <Link href="/" className="hover:text-indigo-600">Experiments</Link>
          <span className="mx-2">/</span>
          <Link href={`/experiments/${params.id}`} className="hover:text-indigo-600">Detail</Link>
          <span className="mx-2">/</span>
          <span className="text-gray-900">Results</span>
        </nav>
        <div className="rounded-md bg-red-50 p-4">
          <p className="text-sm text-red-700">
            {error || 'No analysis results available for this experiment.'}
          </p>
        </div>
      </div>
    );
  }

  const hasCupedData = analysisResult.metricResults.some((m) => m.varianceReductionPct > 0);
  const maxVarianceReduction = Math.max(...analysisResult.metricResults.map((m) => m.varianceReductionPct));

  return (
    <div>
      {/* Breadcrumb */}
      <nav className="mb-4 text-sm text-gray-500">
        <Link href="/" className="hover:text-indigo-600">Experiments</Link>
        <span className="mx-2">/</span>
        <Link href={`/experiments/${params.id}`} className="hover:text-indigo-600">Detail</Link>
        <span className="mx-2">/</span>
        <span className="text-gray-900">Results</span>
      </nav>

      <h1 className="mb-6 text-2xl font-bold text-gray-900">Results Dashboard</h1>

      {/* SRM Banner */}
      <SrmBanner srmResult={analysisResult.srmResult} />

      {/* Summary */}
      <ResultsSummary analysisResult={analysisResult} experiment={experiment} />

      {/* CUPED Toggle */}
      {hasCupedData && (
        <CupedToggle
          enabled={showCuped}
          onToggle={() => setShowCuped((prev) => !prev)}
          varianceReductionPct={maxVarianceReduction}
        />
      )}

      {/* Treatment Effects Table */}
      <section className="mb-6">
        <h2 className="mb-3 text-lg font-semibold text-gray-900">Metric Results</h2>
        <TreatmentEffectsTable metricResults={analysisResult.metricResults} showCuped={showCuped} />
      </section>

      {/* Forest Plot */}
      <ForestPlot metricResults={analysisResult.metricResults} showCuped={showCuped} />

      {/* Sequential Boundary Plot */}
      {experiment.sequentialTestConfig && (
        <SequentialBoundaryPlot
          metricResults={analysisResult.metricResults}
          overallAlpha={experiment.sequentialTestConfig.overallAlpha}
        />
      )}
    </div>
  );
}
