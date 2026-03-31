'use client';

import { createContext, useContext, useReducer, type ReactNode } from 'react';
import type {
  ExperimentType, Variant, GuardrailConfig, GuardrailAction, SequentialMethod,
  InterleavingConfig, SessionConfig, BanditExperimentConfig, QoeConfig, MetaConfig,
} from '@/lib/types';
import { generateVariantId } from '@/lib/validation';

export interface WizardState {
  currentStep: number;
  // Step 1: Basics
  name: string;
  description: string;
  ownerEmail: string;
  type: ExperimentType;
  layerId: string;
  targetingRuleId: string;
  isCumulativeHoldout: boolean;
  // Step 2: Type-specific config
  interleavingConfig: InterleavingConfig;
  sessionConfig: SessionConfig;
  banditExperimentConfig: BanditExperimentConfig;
  qoeConfig: QoeConfig;
  metaConfig: MetaConfig;
  // Step 3: Variants
  variants: Variant[];
  // Step 4: Metrics & Guardrails
  primaryMetricId: string;
  secondaryMetricsInput: string;
  guardrails: GuardrailConfig[];
  guardrailAction: GuardrailAction;
  enableSequential: boolean;
  sequentialMethod: SequentialMethod;
  plannedLooks: number;
  overallAlpha: number;
  // Status
  submitting: boolean;
  formError: string | null;
  stepErrors: Record<number, string | null>;
}

export const DEFAULT_INTERLEAVING_CONFIG: InterleavingConfig = {
  method: 'TEAM_DRAFT',
  algorithmIds: ['', ''],
  creditAssignment: 'BINARY_WIN',
  creditMetricEvent: '',
  maxListSize: 10,
};

export const DEFAULT_SESSION_CONFIG: SessionConfig = {
  sessionIdAttribute: '',
  allowCrossSessionVariation: false,
  minSessionsPerUser: 1,
};

export const DEFAULT_BANDIT_CONFIG: BanditExperimentConfig = {
  algorithm: 'THOMPSON_SAMPLING',
  rewardMetricId: '',
  contextFeatureKeys: [],
  minExplorationFraction: 0.1,
  warmupObservations: 100,
};

export const DEFAULT_QOE_CONFIG: QoeConfig = {
  qoeMetrics: [],
  deviceFilter: '',
};

function defaultVariants(): Variant[] {
  return [
    { variantId: generateVariantId(), name: 'control', trafficFraction: 0.5, isControl: true, payloadJson: '{}' },
    { variantId: generateVariantId(), name: 'treatment', trafficFraction: 0.5, isControl: false, payloadJson: '{}' },
  ];
}

export function createInitialState(): WizardState {
  return {
    currentStep: 0,
    name: '',
    description: '',
    ownerEmail: '',
    type: 'AB',
    layerId: '',
    targetingRuleId: '',
    isCumulativeHoldout: false,
    interleavingConfig: { ...DEFAULT_INTERLEAVING_CONFIG, algorithmIds: ['', ''] },
    sessionConfig: { ...DEFAULT_SESSION_CONFIG },
    banditExperimentConfig: { ...DEFAULT_BANDIT_CONFIG, contextFeatureKeys: [] },
    qoeConfig: { ...DEFAULT_QOE_CONFIG, qoeMetrics: [] },
    metaConfig: { variantBanditConfigs: [] },
    variants: defaultVariants(),
    primaryMetricId: '',
    secondaryMetricsInput: '',
    guardrails: [],
    guardrailAction: 'AUTO_PAUSE',
    enableSequential: false,
    sequentialMethod: 'MSPRT',
    plannedLooks: 0,
    overallAlpha: 0.05,
    submitting: false,
    formError: null,
    stepErrors: {},
  };
}

export type WizardAction =
  | { type: 'SET_FIELD'; field: string; value: unknown }
  | { type: 'SET_STEP'; step: number }
  | { type: 'ADD_VARIANT' }
  | { type: 'DISTRIBUTE_VARIANTS' }
  | { type: 'REMOVE_VARIANT'; index: number }
  | { type: 'UPDATE_VARIANT'; index: number; field: keyof Variant; value: string | number | boolean }
  | { type: 'ADD_GUARDRAIL' }
  | { type: 'REMOVE_GUARDRAIL'; index: number }
  | { type: 'UPDATE_GUARDRAIL'; index: number; field: keyof GuardrailConfig; value: string | number }
  | { type: 'SET_STEP_ERROR'; step: number; error: string | null }
  | { type: 'RESET' };

function wizardReducer(state: WizardState, action: WizardAction): WizardState {
  switch (action.type) {
    case 'SET_FIELD':
      return { ...state, [action.field]: action.value };

    case 'SET_STEP':
      return { ...state, currentStep: action.step };

    case 'ADD_VARIANT':
      return {
        ...state,
        variants: [
          ...state.variants,
          { variantId: generateVariantId(), name: '', trafficFraction: 0, isControl: false, payloadJson: '{}' },
        ],
      };

    case 'DISTRIBUTE_VARIANTS': {
      const count = state.variants.length;
      if (count === 0) return state;
      const equalTraffic = Math.floor((1.0 / count) * 1000) / 1000;
      const remainder = Math.round((1.0 - equalTraffic * count) * 1000) / 1000;

      const variants = state.variants.map((v, i) => ({
        ...v,
        trafficFraction: i === count - 1 ? Math.round((equalTraffic + remainder) * 1000) / 1000 : equalTraffic,
      }));
      return {
        ...state,
        variants,
        stepErrors: { ...state.stepErrors, [state.currentStep]: null },
      };
    }

    case 'REMOVE_VARIANT':
      return { ...state, variants: state.variants.filter((_, i) => i !== action.index) };

    case 'UPDATE_VARIANT': {
      const variants = [...state.variants];
      if (action.field === 'isControl' && action.value === true) {
        variants.forEach((v, i) => { variants[i] = { ...v, isControl: i === action.index }; });
      } else {
        variants[action.index] = { ...variants[action.index], [action.field]: action.value };
      }
      return { ...state, variants };
    }

    case 'ADD_GUARDRAIL':
      return {
        ...state,
        guardrails: [...state.guardrails, { metricId: '', threshold: 0, consecutiveBreachesRequired: 1 }],
      };

    case 'REMOVE_GUARDRAIL':
      return { ...state, guardrails: state.guardrails.filter((_, i) => i !== action.index) };

    case 'UPDATE_GUARDRAIL': {
      const guardrails = [...state.guardrails];
      guardrails[action.index] = { ...guardrails[action.index], [action.field]: action.value };
      return { ...state, guardrails };
    }

    case 'SET_STEP_ERROR':
      return { ...state, stepErrors: { ...state.stepErrors, [action.step]: action.error } };

    case 'RESET':
      return createInitialState();

    default:
      return state;
  }
}

interface WizardContextValue {
  state: WizardState;
  dispatch: React.Dispatch<WizardAction>;
}

const WizardContext = createContext<WizardContextValue | null>(null);

export function WizardProvider({ children }: { children: ReactNode }) {
  const [state, dispatch] = useReducer(wizardReducer, undefined, createInitialState);
  return (
    <WizardContext.Provider value={{ state, dispatch }}>
      {children}
    </WizardContext.Provider>
  );
}

export function useWizard(): WizardContextValue {
  const ctx = useContext(WizardContext);
  if (!ctx) throw new Error('useWizard must be used within WizardProvider');
  return ctx;
}

export const WIZARD_STEPS = ['Basics', 'Type Config', 'Variants', 'Metrics & Guardrails', 'Review'] as const;
