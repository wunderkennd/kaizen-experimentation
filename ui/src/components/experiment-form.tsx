'use client';

import { useState, useCallback } from 'react';
import type {
  CreateExperimentRequest,
  ExperimentType,
  GuardrailAction,
  GuardrailConfig,
  SequentialMethod,
  SequentialTestConfig,
  Variant,
} from '@/lib/types';
import { generateVariantId, validateVariants, validateJsonPayload } from '@/lib/validation';
import { TYPE_LABELS, formatPercent } from '@/lib/utils';

interface ExperimentFormProps {
  onSubmit: (req: CreateExperimentRequest) => Promise<void>;
}

const EXPERIMENT_TYPES: ExperimentType[] = [
  'AB', 'MULTIVARIATE', 'INTERLEAVING', 'SESSION_LEVEL',
  'PLAYBACK_QOE', 'MAB', 'CONTEXTUAL_BANDIT', 'CUMULATIVE_HOLDOUT',
];

const SEQUENTIAL_METHODS: SequentialMethod[] = ['MSPRT', 'GST_OBF', 'GST_POCOCK'];

function defaultVariants(): Variant[] {
  return [
    { variantId: generateVariantId(), name: 'control', trafficFraction: 0.5, isControl: true, payloadJson: '{}' },
    { variantId: generateVariantId(), name: 'treatment', trafficFraction: 0.5, isControl: false, payloadJson: '{}' },
  ];
}

export function ExperimentForm({ onSubmit }: ExperimentFormProps) {
  // Core fields
  const [name, setName] = useState('');
  const [description, setDescription] = useState('');
  const [ownerEmail, setOwnerEmail] = useState('');
  const [type, setType] = useState<ExperimentType>('AB');
  const [layerId, setLayerId] = useState('');
  const [primaryMetricId, setPrimaryMetricId] = useState('');
  const [secondaryMetricsInput, setSecondaryMetricsInput] = useState('');
  const [isCumulativeHoldout, setIsCumulativeHoldout] = useState(false);
  const [targetingRuleId, setTargetingRuleId] = useState('');

  // Guardrails
  const [guardrailAction, setGuardrailAction] = useState<GuardrailAction>('AUTO_PAUSE');
  const [guardrails, setGuardrails] = useState<GuardrailConfig[]>([]);

  // Sequential testing
  const [enableSequential, setEnableSequential] = useState(false);
  const [sequentialMethod, setSequentialMethod] = useState<SequentialMethod>('MSPRT');
  const [plannedLooks, setPlannedLooks] = useState(0);
  const [overallAlpha, setOverallAlpha] = useState(0.05);

  // Variants
  const [variants, setVariants] = useState<Variant[]>(defaultVariants);

  // Form state
  const [submitting, setSubmitting] = useState(false);
  const [formError, setFormError] = useState<string | null>(null);

  const addGuardrail = useCallback(() => {
    setGuardrails((prev) => [
      ...prev,
      { metricId: '', threshold: 0, consecutiveBreachesRequired: 1 },
    ]);
  }, []);

  const removeGuardrail = useCallback((index: number) => {
    setGuardrails((prev) => prev.filter((_, i) => i !== index));
  }, []);

  const updateGuardrail = useCallback((index: number, field: keyof GuardrailConfig, value: string | number) => {
    setGuardrails((prev) => {
      const next = [...prev];
      next[index] = { ...next[index], [field]: value };
      return next;
    });
  }, []);

  // Variant management
  const addVariant = useCallback(() => {
    setVariants((prev) => [
      ...prev,
      { variantId: generateVariantId(), name: '', trafficFraction: 0, isControl: false, payloadJson: '{}' },
    ]);
  }, []);

  const removeVariant = useCallback((index: number) => {
    setVariants((prev) => prev.filter((_, i) => i !== index));
  }, []);

  const updateVariant = useCallback((index: number, field: keyof Variant, value: string | number | boolean) => {
    setVariants((prev) => {
      const next = [...prev];
      if (field === 'isControl' && value === true) {
        next.forEach((v, i) => { next[i] = { ...v, isControl: i === index }; });
      } else {
        next[index] = { ...next[index], [field]: value };
      }
      return next;
    });
  }, []);

  const trafficSum = variants.reduce((acc, v) => acc + v.trafficFraction, 0);
  const trafficSumValid = Math.abs(trafficSum - 1.0) < 1e-9;

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setFormError(null);

    // Validate core fields
    if (!name.trim()) { setFormError('Experiment name is required'); return; }
    if (!ownerEmail.trim()) { setFormError('Owner email is required'); return; }
    if (!layerId.trim()) { setFormError('Layer ID is required'); return; }
    if (!primaryMetricId.trim()) { setFormError('Primary metric is required'); return; }

    // Validate variants
    const variantResult = validateVariants(variants, type);
    if (!variantResult.valid) {
      setFormError(variantResult.bannerError || variantResult.errors[0]?.message || 'Invalid variant configuration');
      return;
    }

    // Validate guardrail metric IDs
    for (const g of guardrails) {
      if (!g.metricId.trim()) { setFormError('All guardrail metrics must have an ID'); return; }
    }

    const secondaryMetricIds = secondaryMetricsInput
      .split(',')
      .map((s) => s.trim())
      .filter(Boolean);

    const sequentialTestConfig: SequentialTestConfig | undefined = enableSequential
      ? { method: sequentialMethod, plannedLooks, overallAlpha }
      : undefined;

    const req: CreateExperimentRequest = {
      name: name.trim(),
      description: description.trim(),
      ownerEmail: ownerEmail.trim(),
      type,
      variants,
      layerId: layerId.trim(),
      primaryMetricId: primaryMetricId.trim(),
      secondaryMetricIds,
      guardrailConfigs: guardrails,
      guardrailAction,
      sequentialTestConfig,
      targetingRuleId: targetingRuleId.trim() || undefined,
      isCumulativeHoldout,
    };

    setSubmitting(true);
    try {
      await onSubmit(req);
    } catch (err) {
      setFormError(err instanceof Error ? err.message : 'Failed to create experiment');
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <form onSubmit={handleSubmit} className="space-y-8">
      {formError && (
        <div className="rounded-md bg-red-50 p-3 text-sm text-red-700" role="alert">
          {formError}
        </div>
      )}

      {/* Basic Info */}
      <section>
        <h2 className="mb-4 text-lg font-semibold text-gray-900">Basic Information</h2>
        <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
          <div>
            <label htmlFor="exp-name" className="block text-sm font-medium text-gray-700">
              Name <span className="text-red-500">*</span>
            </label>
            <input
              id="exp-name"
              type="text"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="e.g., homepage_recs_v3"
              className="mt-1 block w-full rounded-md border border-gray-300 px-3 py-2 text-sm shadow-sm focus:border-indigo-500 focus:ring-indigo-500"
            />
          </div>
          <div>
            <label htmlFor="exp-owner" className="block text-sm font-medium text-gray-700">
              Owner Email <span className="text-red-500">*</span>
            </label>
            <input
              id="exp-owner"
              type="email"
              value={ownerEmail}
              onChange={(e) => setOwnerEmail(e.target.value)}
              placeholder="e.g., alice@streamco.com"
              className="mt-1 block w-full rounded-md border border-gray-300 px-3 py-2 text-sm shadow-sm focus:border-indigo-500 focus:ring-indigo-500"
            />
          </div>
          <div className="sm:col-span-2">
            <label htmlFor="exp-description" className="block text-sm font-medium text-gray-700">
              Description
            </label>
            <textarea
              id="exp-description"
              value={description}
              onChange={(e) => setDescription(e.target.value)}
              rows={2}
              placeholder="What hypothesis is this experiment testing?"
              className="mt-1 block w-full rounded-md border border-gray-300 px-3 py-2 text-sm shadow-sm focus:border-indigo-500 focus:ring-indigo-500"
            />
          </div>
          <div>
            <label htmlFor="exp-type" className="block text-sm font-medium text-gray-700">
              Experiment Type <span className="text-red-500">*</span>
            </label>
            <select
              id="exp-type"
              value={type}
              onChange={(e) => setType(e.target.value as ExperimentType)}
              className="mt-1 block w-full rounded-md border border-gray-300 px-3 py-2 text-sm shadow-sm focus:border-indigo-500 focus:ring-indigo-500"
            >
              {EXPERIMENT_TYPES.map((t) => (
                <option key={t} value={t}>{TYPE_LABELS[t]}</option>
              ))}
            </select>
          </div>
          <div>
            <label htmlFor="exp-layer" className="block text-sm font-medium text-gray-700">
              Layer ID <span className="text-red-500">*</span>
            </label>
            <input
              id="exp-layer"
              type="text"
              value={layerId}
              onChange={(e) => setLayerId(e.target.value)}
              placeholder="e.g., layer-homepage"
              className="mt-1 block w-full rounded-md border border-gray-300 px-3 py-2 text-sm shadow-sm focus:border-indigo-500 focus:ring-indigo-500"
            />
          </div>
        </div>
      </section>

      {/* Metrics */}
      <section>
        <h2 className="mb-4 text-lg font-semibold text-gray-900">Metrics</h2>
        <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
          <div>
            <label htmlFor="exp-primary-metric" className="block text-sm font-medium text-gray-700">
              Primary Metric <span className="text-red-500">*</span>
            </label>
            <input
              id="exp-primary-metric"
              type="text"
              value={primaryMetricId}
              onChange={(e) => setPrimaryMetricId(e.target.value)}
              placeholder="e.g., click_through_rate"
              className="mt-1 block w-full rounded-md border border-gray-300 px-3 py-2 text-sm shadow-sm focus:border-indigo-500 focus:ring-indigo-500"
            />
          </div>
          <div>
            <label htmlFor="exp-secondary-metrics" className="block text-sm font-medium text-gray-700">
              Secondary Metrics
            </label>
            <input
              id="exp-secondary-metrics"
              type="text"
              value={secondaryMetricsInput}
              onChange={(e) => setSecondaryMetricsInput(e.target.value)}
              placeholder="Comma-separated, e.g., watch_time, revenue"
              className="mt-1 block w-full rounded-md border border-gray-300 px-3 py-2 text-sm shadow-sm focus:border-indigo-500 focus:ring-indigo-500"
            />
          </div>
        </div>
      </section>

      {/* Variants */}
      <section>
        <h2 className="mb-4 text-lg font-semibold text-gray-900">Variants</h2>
        <div className="overflow-x-auto rounded-lg border border-gray-200 bg-white">
          <table className="min-w-full divide-y divide-gray-200">
            <thead className="bg-gray-50">
              <tr>
                <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">Name</th>
                <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">Traffic</th>
                <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">Control</th>
                <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">Payload JSON</th>
                <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500" />
              </tr>
            </thead>
            <tbody className="divide-y divide-gray-200 bg-white">
              {variants.map((v, i) => (
                <tr key={v.variantId}>
                  <td className="px-4 py-2">
                    <input
                      type="text"
                      value={v.name}
                      onChange={(e) => updateVariant(i, 'name', e.target.value)}
                      aria-label={`Variant ${i + 1} name`}
                      className="w-full rounded border border-gray-300 px-2 py-1 text-sm"
                    />
                  </td>
                  <td className="px-4 py-2">
                    <input
                      type="number"
                      min={0}
                      max={1}
                      step={0.01}
                      value={v.trafficFraction}
                      onChange={(e) => updateVariant(i, 'trafficFraction', parseFloat(e.target.value) || 0)}
                      aria-label={`Variant ${i + 1} traffic`}
                      className="w-24 rounded border border-gray-300 px-2 py-1 text-sm"
                    />
                  </td>
                  <td className="px-4 py-2 text-center">
                    <input
                      type="radio"
                      name="create-control-variant"
                      checked={v.isControl}
                      onChange={() => updateVariant(i, 'isControl', true)}
                      aria-label={`Set ${v.name || `variant ${i + 1}`} as control`}
                    />
                  </td>
                  <td className="px-4 py-2">
                    <textarea
                      value={v.payloadJson}
                      onChange={(e) => updateVariant(i, 'payloadJson', e.target.value)}
                      aria-label={`Variant ${i + 1} payload`}
                      rows={1}
                      className={`w-full rounded border px-2 py-1 font-mono text-xs ${
                        !validateJsonPayload(v.payloadJson) ? 'border-red-500' : 'border-gray-300'
                      }`}
                    />
                  </td>
                  <td className="px-4 py-2">
                    <button
                      type="button"
                      onClick={() => removeVariant(i)}
                      disabled={variants.length <= 2}
                      className="text-sm text-red-600 hover:text-red-800 disabled:cursor-not-allowed disabled:text-gray-400"
                    >
                      Remove
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
        <div className="mt-2 flex items-center justify-between">
          <span className={`text-sm font-medium ${trafficSumValid ? 'text-green-700' : 'text-red-700'}`}>
            Total traffic: {formatPercent(trafficSum)}
          </span>
          <button
            type="button"
            onClick={addVariant}
            className="rounded-md border border-gray-300 bg-white px-3 py-1.5 text-sm font-medium text-gray-700 hover:bg-gray-50"
          >
            Add Variant
          </button>
        </div>
      </section>

      {/* Guardrails */}
      <section>
        <div className="mb-4 flex items-center justify-between">
          <h2 className="text-lg font-semibold text-gray-900">Guardrails</h2>
          <button
            type="button"
            onClick={addGuardrail}
            className="rounded-md border border-gray-300 bg-white px-3 py-1.5 text-sm font-medium text-gray-700 hover:bg-gray-50"
          >
            Add Guardrail
          </button>
        </div>
        {guardrails.length > 0 && (
          <div className="space-y-3">
            {guardrails.map((g, i) => (
              <div key={i} className="flex items-end gap-3 rounded-lg border border-gray-200 bg-white p-3">
                <div className="flex-1">
                  <label className="block text-xs font-medium text-gray-500">Metric ID</label>
                  <input
                    type="text"
                    value={g.metricId}
                    onChange={(e) => updateGuardrail(i, 'metricId', e.target.value)}
                    aria-label={`Guardrail ${i + 1} metric`}
                    className="mt-1 block w-full rounded border border-gray-300 px-2 py-1 text-sm"
                  />
                </div>
                <div className="w-28">
                  <label className="block text-xs font-medium text-gray-500">Threshold</label>
                  <input
                    type="number"
                    step="any"
                    value={g.threshold}
                    onChange={(e) => updateGuardrail(i, 'threshold', parseFloat(e.target.value) || 0)}
                    aria-label={`Guardrail ${i + 1} threshold`}
                    className="mt-1 block w-full rounded border border-gray-300 px-2 py-1 text-sm"
                  />
                </div>
                <div className="w-28">
                  <label className="block text-xs font-medium text-gray-500">Breaches</label>
                  <input
                    type="number"
                    min={1}
                    value={g.consecutiveBreachesRequired}
                    onChange={(e) => updateGuardrail(i, 'consecutiveBreachesRequired', parseInt(e.target.value) || 1)}
                    aria-label={`Guardrail ${i + 1} breaches required`}
                    className="mt-1 block w-full rounded border border-gray-300 px-2 py-1 text-sm"
                  />
                </div>
                <button
                  type="button"
                  onClick={() => removeGuardrail(i)}
                  className="mb-1 text-sm text-red-600 hover:text-red-800"
                >
                  Remove
                </button>
              </div>
            ))}
            <div>
              <label htmlFor="guardrail-action" className="block text-sm font-medium text-gray-700">Action on Breach</label>
              <select
                id="guardrail-action"
                value={guardrailAction}
                onChange={(e) => setGuardrailAction(e.target.value as GuardrailAction)}
                className="mt-1 block w-48 rounded-md border border-gray-300 px-3 py-2 text-sm shadow-sm"
              >
                <option value="AUTO_PAUSE">Auto-Pause</option>
                <option value="ALERT_ONLY">Alert Only</option>
              </select>
            </div>
          </div>
        )}
        {guardrails.length === 0 && (
          <p className="text-sm text-gray-500">No guardrails configured. Click &ldquo;Add Guardrail&rdquo; to add one.</p>
        )}
      </section>

      {/* Sequential Testing */}
      <section>
        <h2 className="mb-4 text-lg font-semibold text-gray-900">Sequential Testing</h2>
        <label className="flex items-center gap-2">
          <input
            type="checkbox"
            checked={enableSequential}
            onChange={(e) => setEnableSequential(e.target.checked)}
            className="rounded border-gray-300"
          />
          <span className="text-sm text-gray-700">Enable sequential testing</span>
        </label>
        {enableSequential && (
          <div className="mt-3 grid grid-cols-1 gap-4 sm:grid-cols-3">
            <div>
              <label htmlFor="seq-method" className="block text-sm font-medium text-gray-700">Method</label>
              <select
                id="seq-method"
                value={sequentialMethod}
                onChange={(e) => setSequentialMethod(e.target.value as SequentialMethod)}
                className="mt-1 block w-full rounded-md border border-gray-300 px-3 py-2 text-sm shadow-sm"
              >
                {SEQUENTIAL_METHODS.map((m) => (
                  <option key={m} value={m}>{m}</option>
                ))}
              </select>
            </div>
            <div>
              <label htmlFor="seq-looks" className="block text-sm font-medium text-gray-700">Planned Looks</label>
              <input
                id="seq-looks"
                type="number"
                min={0}
                value={plannedLooks}
                onChange={(e) => setPlannedLooks(parseInt(e.target.value) || 0)}
                className="mt-1 block w-full rounded-md border border-gray-300 px-3 py-2 text-sm shadow-sm"
              />
            </div>
            <div>
              <label htmlFor="seq-alpha" className="block text-sm font-medium text-gray-700">Overall Alpha</label>
              <input
                id="seq-alpha"
                type="number"
                min={0}
                max={1}
                step={0.01}
                value={overallAlpha}
                onChange={(e) => setOverallAlpha(parseFloat(e.target.value) || 0.05)}
                className="mt-1 block w-full rounded-md border border-gray-300 px-3 py-2 text-sm shadow-sm"
              />
            </div>
          </div>
        )}
      </section>

      {/* Advanced Options */}
      <section>
        <h2 className="mb-4 text-lg font-semibold text-gray-900">Advanced</h2>
        <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
          <div>
            <label htmlFor="exp-targeting" className="block text-sm font-medium text-gray-700">
              Targeting Rule ID
            </label>
            <input
              id="exp-targeting"
              type="text"
              value={targetingRuleId}
              onChange={(e) => setTargetingRuleId(e.target.value)}
              placeholder="Optional"
              className="mt-1 block w-full rounded-md border border-gray-300 px-3 py-2 text-sm shadow-sm"
            />
          </div>
          <div className="flex items-end">
            <label className="flex items-center gap-2">
              <input
                type="checkbox"
                checked={isCumulativeHoldout}
                onChange={(e) => setIsCumulativeHoldout(e.target.checked)}
                className="rounded border-gray-300"
              />
              <span className="text-sm text-gray-700">Cumulative holdout experiment</span>
            </label>
          </div>
        </div>
      </section>

      {/* Submit */}
      <div className="flex items-center gap-3 border-t border-gray-200 pt-6">
        <button
          type="submit"
          disabled={submitting}
          className="rounded-md bg-indigo-600 px-4 py-2 text-sm font-medium text-white shadow-sm hover:bg-indigo-500 disabled:opacity-50"
        >
          {submitting ? 'Creating...' : 'Create Experiment'}
        </button>
      </div>
    </form>
  );
}
