import { describe, it, expect } from 'vitest';
import {
  validateFilteredMeanConfig,
  validateCompositeConfig,
  validateWindowedCountConfig,
} from '@/lib/validation';
import {
  CompositeOperator,
  type FilteredMeanConfig,
  type CompositeConfig,
  type CompositeOperand,
  type WindowedCountConfig,
} from '@/lib/types';

// --- ADR-026 Phase 1 validators ---
//
// These mirror the M5 server-side checks (PR #552). Deep checks (allowlist
// parsing, cycle detection, operand existence) live server-side; the client
// validators only guard the cheap structural rules.

describe('validateFilteredMeanConfig', () => {
  const ok = (overrides: Partial<FilteredMeanConfig> = {}): FilteredMeanConfig => ({
    valueColumn: 'duration_ms',
    filterSql: 'event_type = \'play\'',
    ...overrides,
  });

  it('accepts a well-formed config', () => {
    expect(validateFilteredMeanConfig(ok())).toEqual({ valid: true });
  });

  it('rejects an uppercase value_column', () => {
    const r = validateFilteredMeanConfig(ok({ valueColumn: 'Duration' }));
    expect(r.valid).toBe(false);
    expect(r.error).toMatch(/lowercase identifier/);
  });

  it('rejects an empty value_column', () => {
    const r = validateFilteredMeanConfig(ok({ valueColumn: '' }));
    expect(r.valid).toBe(false);
  });

  it('rejects an identifier starting with a digit', () => {
    const r = validateFilteredMeanConfig(ok({ valueColumn: '1bad' }));
    expect(r.valid).toBe(false);
  });

  it('requires a non-empty filter_sql', () => {
    const r = validateFilteredMeanConfig(ok({ filterSql: '   ' }));
    expect(r.valid).toBe(false);
    expect(r.error).toMatch(/required/);
  });

  it('rejects filter_sql exceeding 4096 chars', () => {
    const r = validateFilteredMeanConfig(ok({ filterSql: 'x'.repeat(4097) }));
    expect(r.valid).toBe(false);
    expect(r.error).toMatch(/4096/);
  });

  it('accepts filter_sql of exactly 4096 chars', () => {
    expect(validateFilteredMeanConfig(ok({ filterSql: 'x'.repeat(4096) }))).toEqual({ valid: true });
  });
});

describe('validateCompositeConfig', () => {
  const operand = (overrides: Partial<CompositeOperand> = {}): CompositeOperand => ({
    metricId: 'metric_a',
    weight: 1.0,
    ...overrides,
  });
  const cfg = (overrides: Partial<CompositeConfig> = {}): CompositeConfig => ({
    operator: CompositeOperator.ADD,
    operands: [operand({ metricId: 'metric_a' }), operand({ metricId: 'metric_b' })],
    ...overrides,
  });

  it('accepts ADD with 2 operands', () => {
    expect(validateCompositeConfig(cfg())).toEqual({ valid: true });
  });

  it('accepts ADD with 3+ operands', () => {
    expect(validateCompositeConfig(cfg({ operands: [
      operand({ metricId: 'a' }), operand({ metricId: 'b' }), operand({ metricId: 'c' }),
    ] }))).toEqual({ valid: true });
  });

  it('rejects UNSPECIFIED operator', () => {
    const r = validateCompositeConfig(cfg({ operator: CompositeOperator.UNSPECIFIED }));
    expect(r.valid).toBe(false);
    expect(r.error).toMatch(/operator is required/);
  });

  it('rejects ADD with 1 operand', () => {
    const r = validateCompositeConfig(cfg({ operands: [operand()] }));
    expect(r.valid).toBe(false);
    expect(r.error).toMatch(/ADD requires at least 2 operands/);
  });

  it('rejects SUBTRACT with 3 operands', () => {
    const r = validateCompositeConfig(cfg({
      operator: CompositeOperator.SUBTRACT,
      operands: [operand({ metricId: 'a' }), operand({ metricId: 'b' }), operand({ metricId: 'c' })],
    }));
    expect(r.valid).toBe(false);
    expect(r.error).toMatch(/SUBTRACT requires exactly 2 operands/);
  });

  it('accepts DIVIDE with exactly 2 operands', () => {
    expect(validateCompositeConfig(cfg({ operator: CompositeOperator.DIVIDE }))).toEqual({ valid: true });
  });

  it('rejects DIVIDE with 1 operand', () => {
    const r = validateCompositeConfig(cfg({
      operator: CompositeOperator.DIVIDE,
      operands: [operand()],
    }));
    expect(r.valid).toBe(false);
    expect(r.error).toMatch(/DIVIDE requires exactly 2 operands/);
  });

  it('rejects empty operand metric_id', () => {
    const r = validateCompositeConfig(cfg({
      operands: [operand({ metricId: '' }), operand({ metricId: 'b' })],
    }));
    expect(r.valid).toBe(false);
    expect(r.error).toMatch(/metric_id must not be empty/);
  });

  it('rejects WEIGHTED_SUM with weight=0', () => {
    const r = validateCompositeConfig(cfg({
      operator: CompositeOperator.WEIGHTED_SUM,
      operands: [operand({ metricId: 'a', weight: 0 }), operand({ metricId: 'b', weight: 1 })],
    }));
    expect(r.valid).toBe(false);
    expect(r.error).toMatch(/WEIGHTED_SUM operand weights must be > 0/);
  });

  it('rejects WEIGHTED_SUM with negative weight', () => {
    const r = validateCompositeConfig(cfg({
      operator: CompositeOperator.WEIGHTED_SUM,
      operands: [operand({ metricId: 'a', weight: -0.5 }), operand({ metricId: 'b', weight: 1 })],
    }));
    expect(r.valid).toBe(false);
  });

  it('accepts WEIGHTED_SUM with all positive weights', () => {
    expect(validateCompositeConfig(cfg({
      operator: CompositeOperator.WEIGHTED_SUM,
      operands: [operand({ metricId: 'a', weight: 0.25 }), operand({ metricId: 'b', weight: 0.75 })],
    }))).toEqual({ valid: true });
  });
});

describe('validateWindowedCountConfig', () => {
  const ok = (overrides: Partial<WindowedCountConfig> = {}): WindowedCountConfig => ({
    eventType: 'signup_completed',
    filterSql: '',
    windowHours: 24,
    ...overrides,
  });

  it('accepts a well-formed config', () => {
    expect(validateWindowedCountConfig(ok())).toEqual({ valid: true });
  });

  it('rejects an uppercase event_type', () => {
    const r = validateWindowedCountConfig(ok({ eventType: 'SignupCompleted' }));
    expect(r.valid).toBe(false);
    expect(r.error).toMatch(/lowercase identifier/);
  });

  it('rejects window_hours = 0', () => {
    const r = validateWindowedCountConfig(ok({ windowHours: 0 }));
    expect(r.valid).toBe(false);
    expect(r.error).toMatch(/window_hours must be > 0/);
  });

  it('rejects negative window_hours', () => {
    const r = validateWindowedCountConfig(ok({ windowHours: -1 }));
    expect(r.valid).toBe(false);
  });

  it('accepts window_hours = 8760 (1 year boundary)', () => {
    expect(validateWindowedCountConfig(ok({ windowHours: 8760 }))).toEqual({ valid: true });
  });

  it('rejects window_hours = 8761', () => {
    const r = validateWindowedCountConfig(ok({ windowHours: 8761 }));
    expect(r.valid).toBe(false);
    expect(r.error).toMatch(/8760/);
  });

  it('accepts empty filter_sql (optional for WINDOWED_COUNT)', () => {
    expect(validateWindowedCountConfig(ok({ filterSql: '' }))).toEqual({ valid: true });
  });

  it('rejects filter_sql exceeding 4096 chars', () => {
    const r = validateWindowedCountConfig(ok({ filterSql: 'x'.repeat(4097) }));
    expect(r.valid).toBe(false);
    expect(r.error).toMatch(/4096/);
  });
});
