'use client';

import { useEffect, useState, useCallback } from 'react';
import { useParams, useSearchParams, useRouter } from 'next/navigation';
import dynamic from 'next/dynamic';
import type { Experiment, AnalysisResult } from '@/lib/types';
import { getExperiment, getAnalysisResult, getAdaptiveN, RpcError } from '@/lib/api';
import type { AdaptiveNResult } from '@/lib/types';
import { RetryableError } from '@/components/retryable-error';
import { Breadcrumb } from '@/components/breadcrumb';
import { SrmBanner } from '@/components/srm-banner';
import { ResultsSummary } from '@/components/results-summary';
import { CupedToggle } from '@/components/cuped-toggle';
import { IpwToggle } from '@/components/ipw-toggle';
import { IpwDetailsPanel } from '@/components/ipw-details-panel';
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
const FeedbackLoopTab = dynamic(
  () => import('@/components/feedback-loop-tab').then(m => ({ default: m.FeedbackLoopTab })),
  { ssr: false },
);
const AvlmBoundaryPlot = dynamic(
  () => import('@/components/charts/avlm-boundary-plot').then(m => ({ default: m.AvlmBoundaryPlot })),
  { ssr: false },
);
const AdaptiveNTimeline = dynamic(
  () => import('@/components/adaptive-n-timeline').then(m => ({ default: m.AdaptiveNTimeline })),
  { ssr: false },
);

type AnalysisTab = 'overview' | 'novelty' | 'interference' | 'interleaving' | 'surrogate' | 'holdout' | 'guardrails' | 'qoe' | 'lifecycle' | 'session' | 'feedback';

const VALID_TABS: AnalysisTab[] = ['overview', 'novelty', 'interference', 'interleaving', 'surrogate', 'holdout', 'guardrails', 'qoe', 'lifecycle', 'session', 'feedback'];

export default function ResultsPage() {
  const params = useParams<{ id: string }>();
  const [experiment, setExperiment] = useState<Experiment | null>(null);
  const [analysisResult, setAnalysisResult] = useState<AnalysisResult | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [showCuped, setShowCuped] = useState(false);
  const [showIpw, setShowIpw] = useState(false);
  const [adaptiveN, setAdaptiveN] = useState<AdaptiveNResult | null>(null);
  const searchParams = useSearchParams();
  const router = useRouter();
  const rawTab = searchParams.get('tab');
  const initialTab: AnalysisTab = rawTab && VALID_TABS.includes(rawTab as AnalysisTab) ? (rawTab as AnalysisTab) : 'overview';
  const [activeTab, setActiveTabState] = useState<AnalysisTab>(initialTab);

  // Sync activeTab when URL changes via browser back/forward navigation.
  // We listen to popstate (fired by browser back/forward) instead of watching
  // searchParams, because searchParams may return a new reference on every
  // render in some environments, clobbering programmatic tab changes.
  useEffect(() => {
    const handlePopState = () => {
      const url = new URL(window.location.href);
      const tab = url.searchParams.get('tab');
      const validated = tab && VALID_TABS.includes(tab as AnalysisTab) ? (tab as AnalysisTab) : 'overview';
      setActiveTabState(validated);
    };
    window.addEventListener('popstate', handlePopState);
    return () => window.removeEventListener('popstate', handlePopState);
  }, []);

  const setActiveTab = useCallback((tab: AnalysisTab) => {
    setActiveTabState(tab);
    const url = new URL(window.location.href);
    if (tab === 'overview') {
      url.searchParams.delete('tab');
    } else {
      url.searchParams.set('tab', tab);
    }
    router.replace(url.pathname + url.search, { scroll: false });
  }, [router]);

  const fetchData = useCallback(() => {
    if (!params.id) return;
    setLoading(true);
    setError(null);
    setAnalysisResult(null);
    getExperiment(params.id)
      .then((exp) => {
        setExperiment(exp);
        // Fetch analysis result and adaptive-N in parallel
        return Promise.all([
          getAnalysisResult(params.id).catch((err) => {
            if (err instanceof RpcError && err.status === 404) return null;
            throw err;
          }),
          getAdaptiveN(params.id).catch(() => null), // best-effort; 404 = not applicable
        ]);
      })
      .then(([analysis, adaptiveNResult]) => {
        if (analysis) setAnalysisResult(analysis);
        setAdaptiveN(adaptiveNResult);
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

  if (error) {
    return (
      <div>
        <Breadcrumb items={[
          { label: 'Experiments', href: '/' },
          { label: 'Detail', href: `/experiments/${params.id}` },
          { label: 'Results' },
        ]} />
        <RetryableError
          message={error}
          onRetry={fetchData}
          context="analysis results"
        />
      </div>
    );
  }

  if (!experiment || !analysisResult) {
    const isRunning = experiment?.state === 'RUNNING' || experiment?.state === 'STARTING';
    return (
      <div>
        <Breadcrumb items={[
          { label: 'Experiments', href: '/' },
          { label: 'Detail', href: `/experiments/${params.id}` },
          { label: 'Results' },
        ]} />
        <div className="rounded-lg border border-gray-200 bg-white p-8 text-center" data-testid="no-results-yet">
          <svg className="mx-auto h-12 w-12 text-gray-400" fill="none" viewBox="0 0 24 24" strokeWidth={1.5} stroke="currentColor" aria-hidden="true">
            <path strokeLinecap="round" strokeLinejoin="round" d="M3 13.125C3 12.504 3.504 12 4.125 12h2.25c.621 0 1.125.504 1.125 1.125v6.75C7.5 20.496 6.996 21 6.375 21h-2.25A1.125 1.125 0 0 1 3 19.875v-6.75ZM9.75 8.625c0-.621.504-1.125 1.125-1.125h2.25c.621 0 1.125.504 1.125 1.125v11.25c0 .621-.504 1.125-1.125 1.125h-2.25a1.125 1.125 0 0 1-1.125-1.125V8.625ZM16.5 4.125c0-.621.504-1.125 1.125-1.125h2.25C20.496 3 21 3.504 21 4.125v15.75c0 .621-.504 1.125-1.125 1.125h-2.25a1.125 1.125 0 0 1-1.125-1.125V4.125Z" />
          </svg>
          <h3 className="mt-4 text-lg font-semibold text-gray-900">
            {isRunning ? 'Analysis in progress' : 'No results yet'}
          </h3>
          <p className="mt-2 text-sm text-gray-500">
            {isRunning
              ? 'This experiment is still running. Results will appear here once enough data has been collected and the analysis pipeline completes.'
              : 'No analysis results are available for this experiment yet.'}
          </p>
          <button
            type="button"
            onClick={fetchData}
            className="mt-4 rounded-md bg-indigo-600 px-3 py-2 text-sm font-medium text-white hover:bg-indigo-500"
          >
            {isRunning ? 'Check again' : 'Retry'}
          </button>
        </div>
      </div>
    );
  }

  const hasCupedData = analysisResult.metricResults.some((m) => m.varianceReductionPct > 0);
  const maxVarianceReduction = Math.max(...analysisResult.metricResults.map((m) => m.varianceReductionPct));
  const hasIpwData = analysisResult.metricResults.some((m) => m.ipwResult);
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
  // Feedback loop tab is always available for bandit/MAB experiments or any experiment with analysis
  if (experiment.type === 'MAB' || experiment.type === 'CONTEXTUAL_BANDIT' || experiment.type === 'AB') {
    tabs.push({ key: 'feedback', label: 'Feedback Loop' });
  }

  // Fall back to overview if activeTab isn't in the dynamic tab list for this experiment
  // (e.g. ?tab=holdout for a non-holdout experiment)
  const effectiveTab = tabs.some(t => t.key === activeTab) ? activeTab : 'overview';

  return (
    <div>
      <Breadcrumb items={[
        { label: 'Experiments', href: '/' },
        { label: 'Detail', href: `/experiments/${params.id}` },
        { label: 'Results' },
      ]} />

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
                effectiveTab === tab.key
                  ? 'border-indigo-600 text-indigo-600'
                  : 'border-transparent text-gray-500 hover:border-gray-300 hover:text-gray-700'
              }`}
              aria-selected={effectiveTab === tab.key}
              aria-controls={`tabpanel-${tab.key}`}
              role="tab"
            >
              {tab.label}
            </button>
          ))}
        </nav>
      </div>

      {/* Tab content */}
      {effectiveTab === 'overview' && (
        <div role="tabpanel" id="tabpanel-overview" aria-labelledby="tab-overview" tabIndex={0}>
          {/* Adjustment Toggles */}
          <div className="flex flex-wrap items-start gap-6">
            {hasCupedData && (
              <CupedToggle
                enabled={showCuped}
                onToggle={() => setShowCuped((prev) => !prev)}
                varianceReductionPct={maxVarianceReduction}
              />
            )}
            {hasIpwData && (
              <IpwToggle
                enabled={showIpw}
                onToggle={() => setShowIpw((prev) => !prev)}
              />
            )}
          </div>

          {/* Treatment Effects Table */}
          <section className="mb-6">
            <h2 className="mb-3 text-lg font-semibold text-gray-900">Metric Results</h2>
            <TreatmentEffectsTable metricResults={analysisResult.metricResults} showCuped={showCuped} showIpw={showIpw} />
          </section>

          {/* IPW Details Panel (when IPW data exists) */}
          {hasIpwData && <IpwDetailsPanel metricResults={analysisResult.metricResults} />}

          {/* Forest Plot */}
          <ForestPlot metricResults={analysisResult.metricResults} showCuped={showCuped} showIpw={showIpw} />

          {/* Adaptive N timeline (PROMISING zone only) */}
          {adaptiveN && adaptiveN.zone === 'PROMISING' && (
            <section className="mb-6">
              <AdaptiveNTimeline result={adaptiveN} />
            </section>
          )}

          {/* AVLM Confidence Sequence (ADR-015) — replaces separate mSPRT/CUPED views */}
          {experiment.sequentialTestConfig && (
            <section className="mb-6">
              <h2 className="mb-3 text-lg font-semibold text-gray-900">
                Sequential Analysis (AVLM)
              </h2>
              <p className="mb-3 text-sm text-gray-500">
                Anytime-Valid Linear Models unify CUPED variance reduction with sequential
                testing in a single confidence sequence. The shaded band is the 95% confidence
                sequence; when it excludes zero the experiment is conclusive.
              </p>
              {analysisResult.metricResults.map((m) => (
                <div key={m.metricId} className="mb-4">
                  <AvlmBoundaryPlot
                    experimentId={params.id}
                    metricId={m.metricId}
                  />
                </div>
              ))}

              {/* Legacy alpha-spending summary (keep for experiments that haven't migrated to AVLM) */}
              <details className="mt-2">
                <summary className="cursor-pointer text-xs text-gray-400 hover:text-gray-600">
                  Show alpha-spending summary (legacy)
                </summary>
                <div className="mt-2">
                  <SequentialBoundaryPlot
                    metricResults={analysisResult.metricResults}
                    overallAlpha={experiment.sequentialTestConfig.overallAlpha}
                  />
                </div>
              </details>

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
            </section>
          )}
        </div>
      )}

      {effectiveTab === 'novelty' && (
        <div role="tabpanel" id="tabpanel-novelty" aria-labelledby="tab-novelty" tabIndex={0}>
          <NoveltyTab experimentId={params.id} />
        </div>
      )}

      {effectiveTab === 'interference' && (
        <div role="tabpanel" id="tabpanel-interference" aria-labelledby="tab-interference" tabIndex={0}>
          <InterferenceTab experimentId={params.id} />
        </div>
      )}

      {effectiveTab === 'interleaving' && (
        <div role="tabpanel" id="tabpanel-interleaving" aria-labelledby="tab-interleaving" tabIndex={0}>
          <InterleavingTab experimentId={params.id} />
        </div>
      )}

      {effectiveTab === 'lifecycle' && (
        <div role="tabpanel" id="tabpanel-lifecycle" aria-labelledby="tab-lifecycle" tabIndex={0}>
          <CateTab experimentId={params.id} />
        </div>
      )}

      {effectiveTab === 'session' && (
        <div role="tabpanel" id="tabpanel-session" aria-labelledby="tab-session" tabIndex={0}>
          <SessionLevelTab
            metricResults={analysisResult.metricResults.filter(m => m.sessionLevelResult)}
          />
        </div>
      )}

      {effectiveTab === 'surrogate' && analysisResult.surrogateProjections && (
        <div role="tabpanel" id="tabpanel-surrogate" aria-labelledby="tab-surrogate" tabIndex={0}>
          <SurrogateTab projections={analysisResult.surrogateProjections} />
        </div>
      )}

      {effectiveTab === 'holdout' && (
        <div role="tabpanel" id="tabpanel-holdout" aria-labelledby="tab-holdout" tabIndex={0}>
          <HoldoutTab experimentId={params.id} />
        </div>
      )}

      {effectiveTab === 'guardrails' && (
        <div role="tabpanel" id="tabpanel-guardrails" aria-labelledby="tab-guardrails" tabIndex={0}>
          <GuardrailTab experimentId={params.id} />
        </div>
      )}

      {effectiveTab === 'qoe' && (
        <div role="tabpanel" id="tabpanel-qoe" aria-labelledby="tab-qoe" tabIndex={0}>
          <QoeTab experimentId={params.id} />
        </div>
      )}

      {effectiveTab === 'feedback' && (
        <div role="tabpanel" id="tabpanel-feedback" aria-labelledby="tab-feedback" tabIndex={0}>
          <FeedbackLoopTab experimentId={params.id} />
        </div>
      )}
    </div>
  );
}
