import type {
  ExperimentType, Variant,
  InterleavingConfig, SessionConfig, BanditExperimentConfig, QoeConfig,
  GuardrailConfig,
} from './types';

const EPSILON = 1e-9;

/** Check whether variant traffic fractions sum to 1.0 (within float tolerance). */
export function validateTrafficSum(variants: Variant[]): boolean {
  const sum = variants.reduce((acc, v) => acc + v.trafficFraction, 0);
  return Math.abs(sum - 1.0) < EPSILON;
}

/** Return true if the string is valid JSON. */
export function validateJsonPayload(json: string): boolean {
  try {
    JSON.parse(json);
    return true;
  } catch {
    return false;
  }
}

/** Return true if exactly one variant has isControl = true. */
export function hasExactlyOneControl(variants: Variant[]): boolean {
  return variants.filter((v) => v.isControl).length === 1;
}

/** Minimum variant count by experiment type. Bandits need at least 1 arm. */
export function getMinVariants(type: ExperimentType): number {
  switch (type) {
    case 'MAB':
    case 'CONTEXTUAL_BANDIT':
      return 1;
    default:
      return 2;
  }
}

/** Generate a unique variant ID. */
export function generateVariantId(): string {
  return crypto.randomUUID();
}

export interface VariantError {
  index: number;
  field: 'name' | 'trafficFraction' | 'payloadJson';
  message: string;
}

export interface ValidationResult {
  valid: boolean;
  errors: VariantError[];
  bannerError?: string;
}

// --- Wizard step validators ---

export interface StepValidation {
  valid: boolean;
  error?: string;
}

interface BasicsFields {
  name: string;
  ownerEmail: string;
  layerId: string;
}

export function validateBasics(fields: BasicsFields): StepValidation {
  if (!fields.name.trim()) return { valid: false, error: 'Experiment name is required' };
  if (!fields.ownerEmail.trim()) return { valid: false, error: 'Owner email is required' };
  if (!fields.layerId.trim()) return { valid: false, error: 'Layer ID is required' };
  return { valid: true };
}

export function validateInterleavingConfig(config: InterleavingConfig): StepValidation {
  const minAlgorithms = config.method === 'MULTILEAVE' ? 3 : 2;
  if (config.algorithmIds.filter(Boolean).length < minAlgorithms) {
    return { valid: false, error: `At least ${minAlgorithms} algorithm IDs required for ${config.method}` };
  }
  if (!config.creditMetricEvent.trim()) {
    return { valid: false, error: 'Credit metric event is required' };
  }
  if (config.maxListSize < 1) {
    return { valid: false, error: 'Max list size must be at least 1' };
  }
  return { valid: true };
}

export function validateSessionConfig(config: SessionConfig): StepValidation {
  if (!config.sessionIdAttribute.trim()) {
    return { valid: false, error: 'Session ID attribute is required' };
  }
  if (config.minSessionsPerUser < 1) {
    return { valid: false, error: 'Minimum sessions per user must be at least 1' };
  }
  return { valid: true };
}

export function validateBanditConfig(config: BanditExperimentConfig, isContextual: boolean): StepValidation {
  if (!config.rewardMetricId.trim()) {
    return { valid: false, error: 'Reward metric ID is required' };
  }
  if (isContextual && config.contextFeatureKeys.filter(Boolean).length === 0) {
    return { valid: false, error: 'At least one context feature key is required for contextual bandits' };
  }
  if (config.minExplorationFraction < 0 || config.minExplorationFraction > 1) {
    return { valid: false, error: 'Exploration fraction must be between 0 and 1' };
  }
  if (config.warmupObservations < 0) {
    return { valid: false, error: 'Warmup observations must be non-negative' };
  }
  return { valid: true };
}

export function validateQoeConfig(config: QoeConfig): StepValidation {
  if (config.qoeMetrics.length === 0) {
    return { valid: false, error: 'At least one QoE metric must be selected' };
  }
  return { valid: true };
}

export function validateTypeConfig(type: ExperimentType, configs: {
  interleavingConfig: InterleavingConfig;
  sessionConfig: SessionConfig;
  banditExperimentConfig: BanditExperimentConfig;
  qoeConfig: QoeConfig;
}): StepValidation {
  switch (type) {
    case 'INTERLEAVING':
      return validateInterleavingConfig(configs.interleavingConfig);
    case 'SESSION_LEVEL':
      return validateSessionConfig(configs.sessionConfig);
    case 'MAB':
      return validateBanditConfig(configs.banditExperimentConfig, false);
    case 'CONTEXTUAL_BANDIT':
      return validateBanditConfig(configs.banditExperimentConfig, true);
    case 'PLAYBACK_QOE':
      return validateQoeConfig(configs.qoeConfig);
    default:
      return { valid: true };
  }
}

interface MetricsFields {
  primaryMetricId: string;
  guardrails: GuardrailConfig[];
}

export function validateMetricsStep(fields: MetricsFields): StepValidation {
  if (!fields.primaryMetricId.trim()) {
    return { valid: false, error: 'Primary metric is required' };
  }
  for (const g of fields.guardrails) {
    if (!g.metricId.trim()) {
      return { valid: false, error: 'All guardrail metrics must have an ID' };
    }
  }
  return { valid: true };
}

/** Full validation for a variant set. Returns field-level and banner errors. */
export function validateVariants(
  variants: Variant[],
  experimentType: ExperimentType,
): ValidationResult {
  const errors: VariantError[] = [];
  let bannerError: string | undefined;

  // Per-variant field checks
  variants.forEach((v, i) => {
    if (!v.name.trim()) {
      errors.push({ index: i, field: 'name', message: 'Variant name is required' });
    }
    if (v.trafficFraction < 0 || v.trafficFraction > 1) {
      errors.push({ index: i, field: 'trafficFraction', message: 'Traffic must be between 0 and 1' });
    }
    if (!validateJsonPayload(v.payloadJson)) {
      errors.push({ index: i, field: 'payloadJson', message: 'Invalid JSON' });
    }
  });

  // Unique names
  const names = variants.map((v) => v.name.trim()).filter(Boolean);
  if (new Set(names).size !== names.length) {
    const seen = new Set<string>();
    variants.forEach((v, i) => {
      const trimmed = v.name.trim();
      if (trimmed && seen.has(trimmed)) {
        errors.push({ index: i, field: 'name', message: 'Variant names must be unique' });
      }
      seen.add(trimmed);
    });
  }

  // Traffic sum
  if (!validateTrafficSum(variants)) {
    bannerError = 'Traffic fractions must sum to 100%';
  }

  // Exactly one control
  if (!hasExactlyOneControl(variants)) {
    bannerError = bannerError || 'Exactly one variant must be the control';
  }

  return {
    valid: errors.length === 0 && !bannerError,
    errors,
    bannerError,
  };
}
