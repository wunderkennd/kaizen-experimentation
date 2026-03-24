'use client';

import { useCallback } from 'react';
import type {
  CreateExperimentRequest,
  SequentialTestConfig,
} from '@/lib/types';
import {
  validateBasics,
  validateTypeConfig,
  validateVariants,
  validateMetricsStep,
} from '@/lib/validation';
import {
  WizardProvider,
  useWizard,
  WIZARD_STEPS,
} from './wizard/wizard-context';
import { StepIndicator } from './wizard/step-indicator';
import { BasicsStep } from './wizard/steps/basics-step';
import { TypeConfigStep } from './wizard/steps/type-config-step';
import { VariantsStep } from './wizard/steps/variants-step';
import { MetricsStep } from './wizard/steps/metrics-step';
import { ReviewStep } from './wizard/steps/review-step';

interface ExperimentFormProps {
  onSubmit: (req: CreateExperimentRequest) => Promise<void>;
}

function WizardOrchestrator({ onSubmit }: ExperimentFormProps) {
  const { state, dispatch } = useWizard();
  const { currentStep } = state;
  const isLastStep = currentStep === WIZARD_STEPS.length - 1;

  const validateCurrentStep = useCallback((): boolean => {
    let result: { valid: boolean; error?: string } | ReturnType<typeof validateVariants>;
    switch (currentStep) {
      case 0:
        result = validateBasics(state);
        if (!result.valid) {
          dispatch({ type: 'SET_STEP_ERROR', step: 0, error: result.error ?? null });
          return false;
        }
        break;
      case 1:
        result = validateTypeConfig(state.type, state);
        if (!result.valid) {
          dispatch({ type: 'SET_STEP_ERROR', step: 1, error: result.error ?? null });
          return false;
        }
        break;
      case 2: {
        const vr = validateVariants(state.variants, state.type);
        if (!vr.valid) {
          dispatch({ type: 'SET_STEP_ERROR', step: 2, error: vr.bannerError || vr.errors[0]?.message || 'Invalid variant configuration' });
          return false;
        }
        break;
      }
      case 3:
        result = validateMetricsStep(state);
        if (!result.valid) {
          dispatch({ type: 'SET_STEP_ERROR', step: 3, error: result.error ?? null });
          return false;
        }
        break;
    }
    dispatch({ type: 'SET_STEP_ERROR', step: currentStep, error: null });
    return true;
  }, [currentStep, state, dispatch]);

  const goNext = useCallback(() => {
    if (validateCurrentStep() && currentStep < WIZARD_STEPS.length - 1) {
      dispatch({ type: 'SET_STEP', step: currentStep + 1 });
    }
  }, [validateCurrentStep, currentStep, dispatch]);

  const goBack = useCallback(() => {
    if (currentStep > 0) {
      dispatch({ type: 'SET_STEP', step: currentStep - 1 });
    }
  }, [currentStep, dispatch]);

  const handleSubmit = useCallback(async (e: React.FormEvent) => {
    e.preventDefault();
    dispatch({ type: 'SET_FIELD', field: 'formError', value: null });

    const secondaryMetricIds = state.secondaryMetricsInput
      .split(',')
      .map((s) => s.trim())
      .filter(Boolean);

    const sequentialTestConfig: SequentialTestConfig | undefined = state.enableSequential
      ? { method: state.sequentialMethod, plannedLooks: state.plannedLooks, overallAlpha: state.overallAlpha }
      : undefined;

    // Build type-specific config
    const typeConfigs: Pick<CreateExperimentRequest, 'interleavingConfig' | 'sessionConfig' | 'banditExperimentConfig' | 'qoeConfig' | 'metaConfig'> = {};
    switch (state.type) {
      case 'INTERLEAVING':
        typeConfigs.interleavingConfig = {
          ...state.interleavingConfig,
          algorithmIds: state.interleavingConfig.algorithmIds.filter(Boolean),
        };
        break;
      case 'SESSION_LEVEL':
        typeConfigs.sessionConfig = state.sessionConfig;
        break;
      case 'MAB':
      case 'CONTEXTUAL_BANDIT':
        typeConfigs.banditExperimentConfig = {
          ...state.banditExperimentConfig,
          contextFeatureKeys: state.banditExperimentConfig.contextFeatureKeys.filter(Boolean),
        };
        break;
      case 'PLAYBACK_QOE':
        typeConfigs.qoeConfig = state.qoeConfig;
        break;
      case 'META':
        typeConfigs.metaConfig = state.metaConfig;
        break;
    }

    const req: CreateExperimentRequest = {
      name: state.name.trim(),
      description: state.description.trim(),
      ownerEmail: state.ownerEmail.trim(),
      type: state.type,
      variants: state.variants,
      layerId: state.layerId.trim(),
      primaryMetricId: state.primaryMetricId.trim(),
      secondaryMetricIds,
      guardrailConfigs: state.guardrails,
      guardrailAction: state.guardrailAction,
      sequentialTestConfig,
      targetingRuleId: state.targetingRuleId.trim() || undefined,
      isCumulativeHoldout: state.isCumulativeHoldout,
      ...typeConfigs,
    };

    dispatch({ type: 'SET_FIELD', field: 'submitting', value: true });
    try {
      await onSubmit(req);
    } catch (err) {
      dispatch({ type: 'SET_FIELD', field: 'formError', value: err instanceof Error ? err.message : 'Failed to create experiment' });
    } finally {
      dispatch({ type: 'SET_FIELD', field: 'submitting', value: false });
    }
  }, [onSubmit, state, dispatch]);

  const stepError = state.stepErrors[currentStep];

  return (
    <form onSubmit={handleSubmit}>
      <StepIndicator currentStep={currentStep} />

      {(state.formError || stepError) && (
        <div className="mb-4 rounded-md bg-red-50 p-3 text-sm text-red-700" role="alert">
          {state.formError || stepError}
        </div>
      )}

      <div className="mb-6">
        {currentStep === 0 && <BasicsStep />}
        {currentStep === 1 && <TypeConfigStep />}
        {currentStep === 2 && <VariantsStep />}
        {currentStep === 3 && <MetricsStep />}
        {currentStep === 4 && <ReviewStep />}
      </div>

      <div className="flex items-center gap-3 border-t border-gray-200 pt-6">
        {currentStep > 0 && (
          <button
            type="button"
            onClick={goBack}
            className="rounded-md border border-gray-300 bg-white px-4 py-2 text-sm font-medium text-gray-700 shadow-sm hover:bg-gray-50"
          >
            Back
          </button>
        )}
        {!isLastStep ? (
          <button
            type="button"
            onClick={goNext}
            className="rounded-md bg-indigo-600 px-4 py-2 text-sm font-medium text-white shadow-sm hover:bg-indigo-500"
          >
            Next
          </button>
        ) : (
          <button
            type="submit"
            disabled={state.submitting}
            className="rounded-md bg-indigo-600 px-4 py-2 text-sm font-medium text-white shadow-sm hover:bg-indigo-500 disabled:opacity-50"
          >
            {state.submitting ? 'Creating...' : 'Create Experiment'}
          </button>
        )}
      </div>
    </form>
  );
}

export function ExperimentForm({ onSubmit }: ExperimentFormProps) {
  return (
    <WizardProvider>
      <WizardOrchestrator onSubmit={onSubmit} />
    </WizardProvider>
  );
}
