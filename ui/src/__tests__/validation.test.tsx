import { describe, it, expect } from 'vitest';
import {
  validateTrafficSum,
  validateJsonPayload,
  hasExactlyOneControl,
  getMinVariants,
  generateVariantId,
  validateVariants,
} from '@/lib/validation';
import type { Variant } from '@/lib/types';

function makeVariant(overrides: Partial<Variant> = {}): Variant {
  return {
    variantId: 'v-test',
    name: 'test',
    trafficFraction: 0.5,
    isControl: false,
    payloadJson: '{}',
    ...overrides,
  };
}

describe('validateTrafficSum', () => {
  it('returns true when fractions sum to exactly 1.0', () => {
    const variants = [
      makeVariant({ trafficFraction: 0.5 }),
      makeVariant({ trafficFraction: 0.5 }),
    ];
    expect(validateTrafficSum(variants)).toBe(true);
  });

  it('returns true with float edge case (0.1 + 0.2 + 0.7)', () => {
    const variants = [
      makeVariant({ trafficFraction: 0.1 }),
      makeVariant({ trafficFraction: 0.2 }),
      makeVariant({ trafficFraction: 0.7 }),
    ];
    expect(validateTrafficSum(variants)).toBe(true);
  });

  it('returns false when fractions do not sum to 1.0', () => {
    const variants = [
      makeVariant({ trafficFraction: 0.3 }),
      makeVariant({ trafficFraction: 0.3 }),
    ];
    expect(validateTrafficSum(variants)).toBe(false);
  });
});

describe('validateJsonPayload', () => {
  it('returns true for valid JSON', () => {
    expect(validateJsonPayload('{"key": "value"}')).toBe(true);
  });

  it('returns true for empty object', () => {
    expect(validateJsonPayload('{}')).toBe(true);
  });

  it('returns false for invalid JSON', () => {
    expect(validateJsonPayload('{bad json')).toBe(false);
  });
});

describe('hasExactlyOneControl', () => {
  it('returns true with exactly one control', () => {
    const variants = [
      makeVariant({ isControl: true }),
      makeVariant({ isControl: false }),
    ];
    expect(hasExactlyOneControl(variants)).toBe(true);
  });

  it('returns false with zero controls', () => {
    const variants = [
      makeVariant({ isControl: false }),
      makeVariant({ isControl: false }),
    ];
    expect(hasExactlyOneControl(variants)).toBe(false);
  });

  it('returns false with two controls', () => {
    const variants = [
      makeVariant({ isControl: true }),
      makeVariant({ isControl: true }),
    ];
    expect(hasExactlyOneControl(variants)).toBe(false);
  });
});

describe('getMinVariants', () => {
  it('returns 2 for AB tests', () => {
    expect(getMinVariants('AB')).toBe(2);
  });

  it('returns 1 for MAB', () => {
    expect(getMinVariants('MAB')).toBe(1);
  });

  it('returns 1 for CONTEXTUAL_BANDIT', () => {
    expect(getMinVariants('CONTEXTUAL_BANDIT')).toBe(1);
  });
});

describe('generateVariantId', () => {
  it('returns a UUID string', () => {
    const id = generateVariantId();
    expect(id).toMatch(
      /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/,
    );
  });
});

describe('validateVariants', () => {
  it('returns valid for correct variant set', () => {
    const variants = [
      makeVariant({ name: 'control', trafficFraction: 0.5, isControl: true }),
      makeVariant({ name: 'treatment', trafficFraction: 0.5, isControl: false }),
    ];
    const result = validateVariants(variants, 'AB');
    expect(result.valid).toBe(true);
    expect(result.errors).toHaveLength(0);
    expect(result.bannerError).toBeUndefined();
  });

  it('catches empty variant name', () => {
    const variants = [
      makeVariant({ name: '', trafficFraction: 0.5, isControl: true }),
      makeVariant({ name: 'treatment', trafficFraction: 0.5, isControl: false }),
    ];
    const result = validateVariants(variants, 'AB');
    expect(result.valid).toBe(false);
    expect(result.errors).toContainEqual(
      expect.objectContaining({ index: 0, field: 'name', message: 'Variant name is required' }),
    );
  });

  it('catches duplicate variant names', () => {
    const variants = [
      makeVariant({ name: 'same', trafficFraction: 0.5, isControl: true }),
      makeVariant({ name: 'same', trafficFraction: 0.5, isControl: false }),
    ];
    const result = validateVariants(variants, 'AB');
    expect(result.valid).toBe(false);
    expect(result.errors).toContainEqual(
      expect.objectContaining({ field: 'name', message: 'Variant names must be unique' }),
    );
  });
});
