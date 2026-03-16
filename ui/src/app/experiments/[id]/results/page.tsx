'use client';

import { useEffect, useState, useCallback } from 'react';
import { useParams } from 'next/navigation';
import Link from 'next/link';
import dynamic from 'next/dynamic';
import type { Experiment, AnalysisResult } from '@/lib/types';
import { getExperiment, getAnalysisResult } from '@/lib/api';
import { RetryableError } from '@/components/retryable-error';
import { SrmBanner } from '@/components/srm-banner';
import { ResultsSummary } from '@/components/results-summary';
import { CupedToggle } from '@/components/cuped-toggle';
import { TreatmentEffectsTable } from '@/components/treatment-effects-table';

// Dynamic imports — recharts + tab components only load when their tab is active
const ForestPlot = dynamic(
  () => import('@/components/charts/forest-plot').then(m => ({ default: m.ForestPlot })),
  { ssr: false },
);
const SequentialBoundaryPlot = dynamic(
  () => import('@/components/charts/sequential-boundary-plot').then(m => ({ default: m.SequentialBoundaryPlot })),
  { ssr: false },
);
const GstTrajectoryChart = dynamic(
  () => import('@/components/charts/gst-trajectory-chart').then(m => ({ default: m.GstTrajectoryChart })),
  { ssr: false },
);
const NoveltyTab = dynamic(
  () => import('@/components/novelty-tab').then(m => ({ default: m.NoveltyTab })),
  { ssr: false },
);
const InterferenceTab = dynamic(
  () => import('@/components/interference-tab').then(m => ({ default: m.InterferenceTab })),
  { ssr: false },
);
const InterleavingTab = dynamic(
  () => import('@/components/interleaving-tab').then(m => ({ default: m.InterleavingTab })),
  { ssr: false },
);
const SurrogateTab = dynamic(
  () => import('@/components/surrogate-tab').then(m => ({ default: m.SurrogateTab })),
  { ssr: false },
);
const HoldoutTab = dynamic(
  () => import('@/components/holdout-tab').then(m => ({ default: m.HoldoutTab })),
  { ssr: false },
);
const GuardrailTab = dynamic(
  () => import('@/components/guardrail-tab').then(m => ({ default: m.GuardrailTab })),
  { ssr: false },
);
const QoeTab = dynamic(
  () => import('@/components/qoe-tab').then(m => ({ default: m.QoeTab })),
  { ssr: false },
);
const CateTab = dynamic(
  () => import('@/components/cate-tab').then(m => ({ default: m.CateTab })),
  { ssr: false },
);
const SessionLevelTab = dynamic(
  () => import('@/components/session-level-tab').then(m => ({ default: m.SessionLevelTab })),
  { ssr: false },
);

type AnalysisTab = 'overview' | 'novelty' | 'interference' | 'interleaving' | 'surrogate' | 'holdout' | 'guardrails' | 'qoe' | 'lifecycle' | 'session';

export default function ResultsPage() {
  const params = useParams<{ id: string }>();
  const [experiment, setExperiment] = useState<Experiment | null>(null);
  const [analysisResult, setAnalysisResult] = useState<AnalysisResult | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [showCuped, setShowCuped] = useState(false);
  const [activeTab, setActiveTab] = useState<AnalysisTab>('overview');

  const fetchData = useCallback(() => {
    if (!params.id) return;
    setLoading(true);
    setError(null);
    Promise.all([getExperiment(params.id), getAnalysisResult(params.id)])
      .then(([exp, analysis]) => {
        setExperiment(exp);
        setAnalysisResult(analysis);
      })
      .catch((err) => setError(err.message))
      .finally(() => setLoading(false));
  }, [params.id]);

  useEffect(() => { fetchData(); }, [fetchData]);

  if (loading) {
    return (
      <div className="flex items-center justify-center py-12" role="status" aria-label="Loading">
        <div className="h-8 w-8 animate-spin rounded-full border-4 border-gray-300 border-t-indigo-600" />
        <span className="sr-only">Loading</span>
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
        <RetryableError
          message={error || 'No analysis results available for this experiment.'}
          onRetry={fetchData}
          context="analysis results"
        />
      </div>
    );
  }

  const hasCupedData = analysisResult.metricResults.some((m) => m.varianceReductionPct > 0);
  const maxVarianceReduction = Math.max(...analysisResult.metricResults.map((m) => m.varianceReductionPct));
  const hasSurrogateProjections = (analysisResult.surrogateProjections?.length ?? 0) > 0;
  const isHoldout = experiment.type === 'CUMULATIVE_HOLDOUT';
  const hasGuardrails = experiment.guardrailConfigs.length > 0;
  const isQoe = experiment.type === 'PLAYBACK_QOE';
  const hasSessionLevel = analysisResult.metricResults.some(m => m.sessionLevelResult);

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
  if (hasSessionLevel) {
    tabs.push({ key: 'session', label: 'Session-Level' });
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
        <nav className="-mb-px flex space-x-6" aria-label="Analysis tabs" role="tablist">
          {tabs.map((tab) => (
            <button
              key={tab.key}
              id={`tab-${tab.key}`}
              onClick={() => setActiveTab(tab.key)}
              className={`whitespace-nowrap border-b-2 pb-3 pt-1 text-sm font-medium transition-colors ${
                activeTab === tab.key
                  ? 'border-indigo-600 text-indigo-600'
                  : 'border-transparent text-gray-500 hover:border-gray-300 hover:text-gray-700'
              }`}
              aria-selected={activeTab === tab.key}
              aria-controls={`tabpanel-${tab.key}`}
              role="tab"
            >
              {tab.label}
            </button>
          ))}
        </nav>
      </div>

      {/* Tab content */}
      {activeTab === 'overview' && (
        <div role="tabpanel" id="tabpanel-overview" aria-labelledby="tab-overview" tabIndex={0}>
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
        </div>
      )}

      {activeTab === 'novelty' && (
        <div role="tabpanel" id="tabpanel-novelty" aria-labelledby="tab-novelty" tabIndex={0}>
          <NoveltyTab experimentId={params.id} />
        </div>
      )}

      {activeTab === 'interference' && (
        <div role="tabpanel" id="tabpanel-interference" aria-labelledby="tab-interference" tabIndex={0}>
          <InterferenceTab experimentId={params.id} />
        </div>
      )}

      {activeTab === 'interleaving' && (
        <div role="tabpanel" id="tabpanel-interleaving" aria-labelledby="tab-interleaving" tabIndex={0}>
          <InterleavingTab experimentId={params.id} />
        </div>
      )}

      {activeTab === 'lifecycle' && (
        <div role="tabpanel" id="tabpanel-lifecycle" aria-labelledby="tab-lifecycle" tabIndex={0}>
          <CateTab experimentId={params.id} />
        </div>
      )}

      {activeTab === 'session' && (
        <div role="tabpanel" id="tabpanel-session" aria-labelledby="tab-session" tabIndex={0}>
          <SessionLevelTab
            metricResults={analysisResult.metricResults.filter(m => m.sessionLevelResult)}
          />
        </div>
      )}

      {activeTab === 'surrogate' && analysisResult.surrogateProjections && (
        <div role="tabpanel" id="tabpanel-surrogate" aria-labelledby="tab-surrogate" tabIndex={0}>
          <SurrogateTab projections={analysisResult.surrogateProjections} />
        </div>
      )}

      {activeTab === 'holdout' && (
        <div role="tabpanel" id="tabpanel-holdout" aria-labelledby="tab-holdout" tabIndex={0}>
          <HoldoutTab experimentId={params.id} />
        </div>
      )}

      {activeTab === 'guardrails' && (
        <div role="tabpanel" id="tabpanel-guardrails" aria-labelledby="tab-guardrails" tabIndex={0}>
          <GuardrailTab experimentId={params.id} />
        </div>
      )}

      {activeTab === 'qoe' && (
        <div role="tabpanel" id="tabpanel-qoe" aria-labelledby="tab-qoe" tabIndex={0}>
          <QoeTab experimentId={params.id} />
        </div>
      )}
    </div>
  );
}
