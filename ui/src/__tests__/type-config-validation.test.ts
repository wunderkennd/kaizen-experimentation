import { describe, it, expect } from 'vitest';
import {
  validateBasics,
  validateInterleavingConfig,
  validateSessionConfig,
  validateBanditConfig,
  validateQoeConfig,
  validateTypeConfig,
  validateMetricsStep,
} from '@/lib/validation';
import type {
  InterleavingConfig, SessionConfig, BanditExperimentConfig, QoeConfig,
} from '@/lib/types';

describe('validateBasics', () => {
  it('fails when name is empty', () => {
    const result = validateBasics({ name: '', ownerEmail: 'a@b.com', layerId: 'layer-1' });
    expect(result.valid).toBe(false);
    expect(result.error).toMatch(/name/i);
  });

  it('fails when ownerEmail is empty', () => {
    const result = validateBasics({ name: 'test', ownerEmail: '', layerId: 'layer-1' });
    expect(result.valid).toBe(false);
    expect(result.error).toMatch(/email/i);
  });

  it('fails when layerId is empty', () => {
    const result = validateBasics({ name: 'test', ownerEmail: 'a@b.com', layerId: '' });
    expect(result.valid).toBe(false);
    expect(result.error).toMatch(/layer/i);
  });

  it('passes with all fields filled', () => {
    const result = validateBasics({ name: 'test', ownerEmail: 'a@b.com', layerId: 'layer-1' });
    expect(result.valid).toBe(true);
  });
});

describe('validateInterleavingConfig', () => {
  const base: InterleavingConfig = {
    method: 'TEAM_DRAFT',
    algorithmIds: ['algo-1', 'algo-2'],
    creditAssignment: 'BINARY_WIN',
    creditMetricEvent: 'click',
    maxListSize: 10,
  };

  it('fails with fewer than 2 algorithm IDs', () => {
    const result = validateInterleavingConfig({ ...base, algorithmIds: ['algo-1'] });
    expect(result.valid).toBe(false);
    expect(result.error).toMatch(/algorithm/i);
  });

  it('fails with fewer than 3 algorithm IDs for MULTILEAVE', () => {
    const result = validateInterleavingConfig({
      ...base,
      method: 'MULTILEAVE',
      algorithmIds: ['algo-1', 'algo-2'],
    });
    expect(result.valid).toBe(false);
    expect(result.error).toContain('3');
  });

  it('passes MULTILEAVE with 3 algorithms', () => {
    const result = validateInterleavingConfig({
      ...base,
      method: 'MULTILEAVE',
      algorithmIds: ['algo-1', 'algo-2', 'algo-3'],
    });
    expect(result.valid).toBe(true);
  });

  it('fails when creditMetricEvent is empty', () => {
    const result = validateInterleavingConfig({ ...base, creditMetricEvent: '' });
    expect(result.valid).toBe(false);
    expect(result.error).toMatch(/credit metric/i);
  });

  it('fails when maxListSize is less than 1', () => {
    const result = validateInterleavingConfig({ ...base, maxListSize: 0 });
    expect(result.valid).toBe(false);
    expect(result.error).toMatch(/list size/i);
  });

  it('passes with valid config', () => {
    expect(validateInterleavingConfig(base).valid).toBe(true);
  });
});

describe('validateSessionConfig', () => {
  const base: SessionConfig = {
    sessionIdAttribute: 'session_id',
    allowCrossSessionVariation: false,
    minSessionsPerUser: 1,
  };

  it('fails when sessionIdAttribute is empty', () => {
    const result = validateSessionConfig({ ...base, sessionIdAttribute: '' });
    expect(result.valid).toBe(false);
    expect(result.error).toMatch(/session/i);
  });

  it('fails when minSessionsPerUser is less than 1', () => {
    const result = validateSessionConfig({ ...base, minSessionsPerUser: 0 });
    expect(result.valid).toBe(false);
    expect(result.error).toMatch(/sessions/i);
  });

  it('passes with valid config', () => {
    expect(validateSessionConfig(base).valid).toBe(true);
  });
});

describe('validateBanditConfig', () => {
  const base: BanditExperimentConfig = {
    algorithm: 'THOMPSON_SAMPLING',
    rewardMetricId: 'conversion_rate',
    contextFeatureKeys: ['genre', 'device'],
    minExplorationFraction: 0.1,
    warmupObservations: 100,
  };

  it('fails when rewardMetricId is empty', () => {
    const result = validateBanditConfig({ ...base, rewardMetricId: '' }, false);
    expect(result.valid).toBe(false);
    expect(result.error).toMatch(/reward/i);
  });

  it('fails for contextual bandit with no context features', () => {
    const result = validateBanditConfig({ ...base, contextFeatureKeys: [] }, true);
    expect(result.valid).toBe(false);
    expect(result.error).toMatch(/context feature/i);
  });

  it('passes for non-contextual bandit with no context features', () => {
    const result = validateBanditConfig({ ...base, contextFeatureKeys: [] }, false);
    expect(result.valid).toBe(true);
  });

  it('fails when explorationFraction is out of range', () => {
    const result = validateBanditConfig({ ...base, minExplorationFraction: 1.5 }, false);
    expect(result.valid).toBe(false);
    expect(result.error).toMatch(/exploration/i);
  });

  it('passes with valid config', () => {
    expect(validateBanditConfig(base, true).valid).toBe(true);
  });
});

describe('validateQoeConfig', () => {
  it('fails with empty metrics array', () => {
    const config: QoeConfig = { qoeMetrics: [], deviceFilter: '' };
    const result = validateQoeConfig(config);
    expect(result.valid).toBe(false);
    expect(result.error).toMatch(/metric/i);
  });

  it('passes with at least one metric', () => {
    const config: QoeConfig = { qoeMetrics: ['rebuffer_ratio'], deviceFilter: '' };
    expect(validateQoeConfig(config).valid).toBe(true);
  });
});

describe('validateTypeConfig (dispatcher)', () => {
  const configs = {
    interleavingConfig: {
      method: 'TEAM_DRAFT' as const,
      algorithmIds: ['a', 'b'],
      creditAssignment: 'BINARY_WIN' as const,
      creditMetricEvent: 'click',
      maxListSize: 10,
    },
    sessionConfig: {
      sessionIdAttribute: 'sid',
      allowCrossSessionVariation: false,
      minSessionsPerUser: 1,
    },
    banditExperimentConfig: {
      algorithm: 'THOMPSON_SAMPLING' as const,
      rewardMetricId: 'ctr',
      contextFeatureKeys: ['genre'],
      minExplorationFraction: 0.1,
      warmupObservations: 100,
    },
    qoeConfig: {
      qoeMetrics: ['rebuffer_ratio'],
      deviceFilter: '',
    },
  };

  it('validates AB type as always valid', () => {
    expect(validateTypeConfig('AB', configs).valid).toBe(true);
  });

  it('validates INTERLEAVING type', () => {
    expect(validateTypeConfig('INTERLEAVING', configs).valid).toBe(true);
  });

  it('validates CONTEXTUAL_BANDIT type', () => {
    expect(validateTypeConfig('CONTEXTUAL_BANDIT', configs).valid).toBe(true);
  });
});

describe('validateMetricsStep', () => {
  it('fails when primaryMetricId is empty', () => {
    const result = validateMetricsStep({ primaryMetricId: '', guardrails: [] });
    expect(result.valid).toBe(false);
    expect(result.error).toMatch(/primary metric/i);
  });

  it('fails when a guardrail has empty metricId', () => {
    const result = validateMetricsStep({
      primaryMetricId: 'ctr',
      guardrails: [{ metricId: '', threshold: 0.01, consecutiveBreachesRequired: 1 }],
    });
    expect(result.valid).toBe(false);
    expect(result.error).toMatch(/guardrail/i);
  });

  it('passes with valid fields', () => {
    const result = validateMetricsStep({
      primaryMetricId: 'ctr',
      guardrails: [{ metricId: 'crash_rate', threshold: 0.01, consecutiveBreachesRequired: 1 }],
    });
    expect(result.valid).toBe(true);
  });
});
