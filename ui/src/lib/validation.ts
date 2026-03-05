import type { ExperimentType, Variant } from './types';

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
