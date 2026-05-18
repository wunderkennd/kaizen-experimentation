import type {
  ExperimentType, Variant,
  InterleavingConfig, SessionConfig, BanditExperimentConfig, QoeConfig,
  GuardrailConfig, MetaConfig,
  FilteredMeanConfig, CompositeConfig, WindowedCountConfig,
} from './types';
import { CompositeOperator } from './types';

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

export function validateMetaConfig(config: MetaConfig): StepValidation {
  if (config.variantBanditConfigs.length === 0) {
    return { valid: false, error: 'At least one variant bandit configuration is required' };
  }
  for (const vc of config.variantBanditConfigs) {
    if (vc.arms.length === 0) {
      return { valid: false, error: `Variant ${vc.variantId} must have at least one arm` };
    }
  }
  return { valid: true };
}

export function validateTypeConfig(type: ExperimentType, configs: {
  interleavingConfig: InterleavingConfig;
  sessionConfig: SessionConfig;
  banditExperimentConfig: BanditExperimentConfig;
  qoeConfig: QoeConfig;
  metaConfig?: MetaConfig;
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
    case 'META':
      return validateMetaConfig(configs.metaConfig ?? { variantBanditConfigs: [] });
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

// --- ADR-026 Phase 1 — custom metric type validators ---
//
// Client-side validators for the three new metric type configs. Deep checks
// (allowlist parsing of `filter_sql`, cycle detection on COMPOSITE operands,
// operand existence) live server-side in M5 — see PR #552 and ADR-026
// BUG-0001/0002 fixes. These hooks just gate the form submission with the
// inexpensive sanity checks that don't require a server round trip.

/**
 * Lowercase identifier regex shared with the M5 Rust validators
 * (`^[a-z_][a-z0-9_]*$`). Mirror this exactly so client- and server-side
 * rejections produce the same message bucket.
 */
const IDENTIFIER_RE = /^[a-z_][a-z0-9_]*$/;

const FILTER_SQL_MAX_LEN = 4096;
const WINDOW_HOURS_MAX = 8760; // 1 year

/**
 * Validate a `FilteredMeanConfig`. `value_column` must be a bare lowercase
 * identifier and `filter_sql` is REQUIRED — for unfiltered means callers
 * should pick `METRIC_TYPE_MEAN` instead.
 */
export function validateFilteredMeanConfig(cfg: FilteredMeanConfig): StepValidation {
  if (!cfg.valueColumn || !IDENTIFIER_RE.test(cfg.valueColumn)) {
    return { valid: false, error: 'value_column must be a lowercase identifier (e.g. duration_ms)' };
  }
  if (!cfg.filterSql || cfg.filterSql.trim().length === 0) {
    return { valid: false, error: 'filter_sql is required for FILTERED_MEAN. Use METRIC_TYPE_MEAN if no filter is needed.' };
  }
  if (cfg.filterSql.length > FILTER_SQL_MAX_LEN) {
    return { valid: false, error: `filter_sql exceeds ${FILTER_SQL_MAX_LEN} character limit` };
  }
  return { valid: true };
}

function operatorName(op: CompositeOperator): string {
  switch (op) {
    case CompositeOperator.UNSPECIFIED: return 'UNSPECIFIED';
    case CompositeOperator.ADD: return 'ADD';
    case CompositeOperator.SUBTRACT: return 'SUBTRACT';
    case CompositeOperator.MULTIPLY: return 'MULTIPLY';
    case CompositeOperator.DIVIDE: return 'DIVIDE';
    case CompositeOperator.WEIGHTED_SUM: return 'WEIGHTED_SUM';
    default: return 'UNKNOWN';
  }
}

/**
 * Validate a `CompositeConfig`. Enforces operator arity and the
 * WEIGHTED_SUM `weight > 0` constraint. Cycle detection and operand
 * existence (does each `metric_id` actually resolve?) are deferred to the
 * server-side handler — those need the metric catalog.
 */
export function validateCompositeConfig(cfg: CompositeConfig): StepValidation {
  if (cfg.operator === CompositeOperator.UNSPECIFIED) {
    return { valid: false, error: 'operator is required' };
  }
  const n = cfg.operands.length;
  switch (cfg.operator) {
    case CompositeOperator.ADD:
    case CompositeOperator.MULTIPLY:
    case CompositeOperator.WEIGHTED_SUM:
      if (n < 2) return { valid: false, error: `${operatorName(cfg.operator)} requires at least 2 operands` };
      break;
    case CompositeOperator.SUBTRACT:
    case CompositeOperator.DIVIDE:
      if (n !== 2) return { valid: false, error: `${operatorName(cfg.operator)} requires exactly 2 operands` };
      break;
  }
  for (const op of cfg.operands) {
    if (!op.metricId || op.metricId.trim().length === 0) {
      return { valid: false, error: 'operand metric_id must not be empty' };
    }
  }
  if (cfg.operator === CompositeOperator.WEIGHTED_SUM) {
    for (const op of cfg.operands) {
      if (!(op.weight > 0)) {
        return { valid: false, error: `WEIGHTED_SUM operand weights must be > 0 (got ${op.weight} for ${op.metricId})` };
      }
    }
  }
  return { valid: true };
}

/**
 * Validate a `WindowedCountConfig`. `event_type` must be a bare lowercase
 * identifier, `window_hours` must fall in (0, 8760], and `filter_sql` is
 * optional but capped at 4096 chars when supplied.
 */
export function validateWindowedCountConfig(cfg: WindowedCountConfig): StepValidation {
  if (!cfg.eventType || !IDENTIFIER_RE.test(cfg.eventType)) {
    return { valid: false, error: 'event_type must be a lowercase identifier (e.g. signup_completed)' };
  }
  if (!(cfg.windowHours > 0)) {
    return { valid: false, error: 'window_hours must be > 0' };
  }
  if (cfg.windowHours > WINDOW_HOURS_MAX) {
    return { valid: false, error: `window_hours must be ≤ ${WINDOW_HOURS_MAX} (1 year)` };
  }
  if (cfg.filterSql && cfg.filterSql.length > FILTER_SQL_MAX_LEN) {
    return { valid: false, error: `filter_sql exceeds ${FILTER_SQL_MAX_LEN} character limit` };
  }
  return { valid: true };
}
