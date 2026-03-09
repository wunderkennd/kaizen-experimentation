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
import { GstTrajectoryChart } from '@/components/charts/gst-trajectory-chart';
import { NoveltyTab } from '@/components/novelty-tab';
import { InterferenceTab } from '@/components/interference-tab';
import { InterleavingTab } from '@/components/interleaving-tab';
import { SurrogateTab } from '@/components/surrogate-tab';
import { HoldoutTab } from '@/components/holdout-tab';
import { GuardrailTab } from '@/components/guardrail-tab';
import { QoeTab } from '@/components/qoe-tab';
import { CateTab } from '@/components/cate-tab';

type AnalysisTab = 'overview' | 'novelty' | 'interference' | 'interleaving' | 'surrogate' | 'holdout' | 'guardrails' | 'qoe' | 'lifecycle';

export default function ResultsPage() {
  const params = useParams<{ id: string }>();
  const [experiment, setExperiment] = useState<Experiment | null>(null);
  const [analysisResult, setAnalysisResult] = useState<AnalysisResult | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [showCuped, setShowCuped] = useState(false);
  const [activeTab, setActiveTab] = useState<AnalysisTab>('overview');

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
  const hasSurrogateProjections = (analysisResult.surrogateProjections?.length ?? 0) > 0;
  const isHoldout = experiment.type === 'CUMULATIVE_HOLDOUT';
  const hasGuardrails = experiment.guardrailConfigs.length > 0;
  const isQoe = experiment.type === 'PLAYBACK_QOE';

  // Build tabs dynamically based on experiment features
  const tabs: { key: AnalysisTab; label: string }[] = [
    { key: 'overview', label: 'Overview' },
    { key: 'novelty', label: 'Novelty Effects' },
    { key: 'interference', label: 'Content Interference' },
    { key: 'interleaving', label: 'Interleaving' },
  ];
  if (isQoe) {
    tabs.push({ key: 'qoe', label: 'QoE Metrics' });
  }
  if (experiment.type === 'AB' || experiment.type === 'MULTIVARIATE') {
    tabs.push({ key: 'lifecycle', label: 'Lifecycle Segments' });
  }
  if (hasSurrogateProjections) {
    tabs.push({ key: 'surrogate', label: 'Surrogate Projections' });
  }
  if (isHoldout) {
    tabs.push({ key: 'holdout', label: 'Holdout Lift' });
  }
  if (hasGuardrails) {
    tabs.push({ key: 'guardrails', label: 'Guardrails' });
  }

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

      {/* Tab navigation */}
      <div className="mb-6 border-b border-gray-200">
        <nav className="-mb-px flex space-x-6" aria-label="Analysis tabs">
          {tabs.map((tab) => (
            <button
              key={tab.key}
              onClick={() => setActiveTab(tab.key)}
              className={`whitespace-nowrap border-b-2 pb-3 pt-1 text-sm font-medium transition-colors ${
                activeTab === tab.key
                  ? 'border-indigo-600 text-indigo-600'
                  : 'border-transparent text-gray-500 hover:border-gray-300 hover:text-gray-700'
              }`}
              aria-selected={activeTab === tab.key}
              role="tab"
            >
              {tab.label}
            </button>
          ))}
        </nav>
      </div>

      {/* Tab content */}
      {activeTab === 'overview' && (
        <>
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
            <>
              <SequentialBoundaryPlot
                metricResults={analysisResult.metricResults}
                overallAlpha={experiment.sequentialTestConfig.overallAlpha}
              />

              {/* GST Stopping Boundary Trajectory */}
              {analysisResult.metricResults
                .filter((m) => m.sequentialResult)
                .map((m) => (
                  <GstTrajectoryChart
                    key={m.metricId}
                    experimentId={params.id}
                    metricId={m.metricId}
                  />
                ))}
            </>
          )}
        </>
      )}

      {activeTab === 'novelty' && (
        <NoveltyTab experimentId={params.id} />
      )}

      {activeTab === 'interference' && (
        <InterferenceTab experimentId={params.id} />
      )}

      {activeTab === 'interleaving' && (
        <InterleavingTab experimentId={params.id} />
      )}

      {activeTab === 'lifecycle' && (
        <CateTab experimentId={params.id} />
      )}

      {activeTab === 'surrogate' && analysisResult.surrogateProjections && (
        <SurrogateTab projections={analysisResult.surrogateProjections} />
      )}

      {activeTab === 'holdout' && (
        <HoldoutTab experimentId={params.id} />
      )}

      {activeTab === 'guardrails' && (
        <GuardrailTab experimentId={params.id} />
      )}

      {activeTab === 'qoe' && (
        <QoeTab experimentId={params.id} />
      )}
    </div>
  );
}
