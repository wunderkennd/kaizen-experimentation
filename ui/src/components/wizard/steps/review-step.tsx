'use client';

import { useWizard } from '../wizard-context';
import { TYPE_LABELS } from '@/lib/utils';
import { formatPercent } from '@/lib/utils';
import { CopyButton } from '@/components/copy-button';

interface ReviewSectionProps {
  title: string;
  step: number;
  onEdit: (step: number) => void;
  children: React.ReactNode;
}

function ReviewSection({ title, step, onEdit, children }: ReviewSectionProps) {
  return (
    <div className="rounded-lg border border-gray-200 bg-white p-4">
      <div className="mb-3 flex items-center justify-between">
        <h3 className="text-sm font-semibold text-gray-900">{title}</h3>
        <button
          type="button"
          onClick={() => onEdit(step)}
          className="text-xs font-medium text-indigo-600 hover:text-indigo-800"
        >
          Edit
        </button>
      </div>
      {children}
    </div>
  );
}

function DlRow({
  label,
  value,
  copyValue,
  isCode,
}: {
  label: string;
  value: string;
  copyValue?: string;
  isCode?: boolean;
}) {
  return (
    <div className="flex gap-2 py-1">
      <dt className="w-40 flex-shrink-0 text-xs font-medium text-gray-500">{label}</dt>
      <dd className="flex items-center gap-2 text-xs text-gray-900">
        {isCode ? (
          <code className="rounded bg-gray-50 px-1 py-0.5 text-[10px] text-gray-600">
            {value || '\u2014'}
          </code>
        ) : (
          <span>{value || '\u2014'}</span>
        )}
        {copyValue && value && (
          <CopyButton
            value={copyValue}
            label={`Copy ${label}`}
            className="h-4 w-4"
            successMessage={`${label} copied`}
          />
        )}
      </dd>
    </div>
  );
}

export function ReviewStep() {
  const { state, dispatch } = useWizard();

  const goToStep = (step: number) => dispatch({ type: 'SET_STEP', step });

  const trafficSum = state.variants.reduce((acc, v) => acc + v.trafficFraction, 0);

  return (
    <section className="space-y-4">
      <h2 className="mb-4 text-lg font-semibold text-gray-900">Review Experiment</h2>

      {/* Basics */}
      <ReviewSection title="Basic Information" step={0} onEdit={goToStep}>
        <dl>
          <DlRow label="Name" value={state.name} />
          <DlRow label="Owner" value={state.ownerEmail} />
          <DlRow label="Type" value={TYPE_LABELS[state.type]} />
          <DlRow
            label="Layer"
            value={state.layerId}
            isCode
            copyValue={state.layerId}
          />
          <DlRow label="Description" value={state.description} />
          {state.targetingRuleId && (
            <DlRow
              label="Targeting Rule"
              value={state.targetingRuleId}
              isCode
              copyValue={state.targetingRuleId}
            />
          )}
          {state.isCumulativeHoldout && <DlRow label="Cumulative Holdout" value="Yes" />}
        </dl>
      </ReviewSection>

      {/* Type Config */}
      <ReviewSection title={`${TYPE_LABELS[state.type]} Config`} step={1} onEdit={goToStep}>
        {renderTypeConfigSummary(state)}
      </ReviewSection>

      {/* Variants */}
      <ReviewSection title="Variants" step={2} onEdit={goToStep}>
        <div className="overflow-x-auto">
          <table className="min-w-full text-xs">
            <thead>
              <tr className="border-b border-gray-100">
                <th className="py-1 pr-4 text-left font-medium text-gray-500">Name</th>
                <th className="py-1 pr-4 text-left font-medium text-gray-500">Traffic</th>
                <th className="py-1 pr-4 text-left font-medium text-gray-500">Control</th>
                <th className="py-1 text-left font-medium text-gray-500">Payload</th>
              </tr>
            </thead>
            <tbody>
              {state.variants.map((v) => (
                <tr key={v.variantId} className="border-b border-gray-50">
                  <td className="py-1 pr-4 text-gray-900">{v.name || '\u2014'}</td>
                  <td className="py-1 pr-4 text-gray-900">{formatPercent(v.trafficFraction)}</td>
                  <td className="py-1 pr-4">{v.isControl ? <span className="rounded bg-blue-100 px-1.5 py-0.5 text-blue-700">Control</span> : '\u2014'}</td>
                  <td className="py-1 font-mono text-gray-600">{v.payloadJson.length > 50 ? v.payloadJson.slice(0, 50) + '\u2026' : v.payloadJson}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
        <p className={`mt-2 text-xs font-medium ${Math.abs(trafficSum - 1.0) < 1e-9 ? 'text-green-700' : 'text-red-700'}`}>
          Total traffic: {formatPercent(trafficSum)}
        </p>
      </ReviewSection>

      {/* Metrics & Guardrails */}
      <ReviewSection title="Metrics & Guardrails" step={3} onEdit={goToStep}>
        <dl>
          <DlRow
            label="Primary Metric"
            value={state.primaryMetricId}
            isCode
            copyValue={state.primaryMetricId}
          />
          <DlRow label="Secondary Metrics" value={state.secondaryMetricsInput || 'None'} />
          {state.guardrails.length > 0 && (
            <DlRow
              label="Guardrails"
              value={state.guardrails.map((g) => `${g.metricId} (${g.threshold})`).join(', ')}
            />
          )}
          {state.guardrails.length > 0 && (
            <DlRow label="Guardrail Action" value={state.guardrailAction === 'AUTO_PAUSE' ? 'Auto-Pause' : 'Alert Only'} />
          )}
          {state.enableSequential && (
            <>
              <DlRow label="Sequential Method" value={state.sequentialMethod} />
              <DlRow label="Planned Looks" value={String(state.plannedLooks)} />
              <DlRow label="Overall Alpha" value={String(state.overallAlpha)} />
            </>
          )}
        </dl>
      </ReviewSection>
    </section>
  );
}

function renderTypeConfigSummary(state: ReturnType<typeof useWizard>['state']) {
  switch (state.type) {
    case 'INTERLEAVING': {
      const c = state.interleavingConfig;
      return (
        <dl>
          <DlRow label="Method" value={c.method} />
          <DlRow label="Algorithm IDs" value={c.algorithmIds.filter(Boolean).join(', ')} />
          <DlRow label="Credit Assignment" value={c.creditAssignment} />
          <DlRow
            label="Credit Metric"
            value={c.creditMetricEvent}
            isCode
            copyValue={c.creditMetricEvent}
          />
          <DlRow label="Max List Size" value={String(c.maxListSize)} />
        </dl>
      );
    }
    case 'SESSION_LEVEL': {
      const c = state.sessionConfig;
      return (
        <dl>
          <DlRow
            label="Session ID Attribute"
            value={c.sessionIdAttribute}
            isCode
            copyValue={c.sessionIdAttribute}
          />
          <DlRow label="Cross-Session" value={c.allowCrossSessionVariation ? 'Yes' : 'No'} />
          <DlRow label="Min Sessions" value={String(c.minSessionsPerUser)} />
        </dl>
      );
    }
    case 'MAB':
    case 'CONTEXTUAL_BANDIT': {
      const c = state.banditExperimentConfig;
      return (
        <dl>
          <DlRow label="Algorithm" value={c.algorithm} />
          <DlRow
            label="Reward Metric"
            value={c.rewardMetricId}
            isCode
            copyValue={c.rewardMetricId}
          />
          {state.type === 'CONTEXTUAL_BANDIT' && (
            <DlRow label="Context Features" value={c.contextFeatureKeys.filter(Boolean).join(', ')} />
          )}
          <DlRow label="Exploration Fraction" value={String(c.minExplorationFraction)} />
          <DlRow label="Warmup Observations" value={String(c.warmupObservations)} />
        </dl>
      );
    }
    case 'PLAYBACK_QOE': {
      const c = state.qoeConfig;
      return (
        <dl>
          <DlRow label="QoE Metrics" value={c.qoeMetrics.join(', ') || 'None selected'} />
          <DlRow label="Device Filter" value={c.deviceFilter || 'None'} />
        </dl>
      );
    }
    default:
      return <p className="text-xs text-gray-500">No type-specific configuration.</p>;
  }
}
