/**
 * M6↔M7 Feature Flag wire-format contract tests.
 *
 * Validates that the UI can correctly consume the M7 FeatureFlagService
 * ConnectRPC JSON wire format:
 *   - FLAG_TYPE_ enum prefix stripping
 *   - Proto3 zero-value omission (false booleans, 0.0 rollout absent)
 *   - FlagVariant nested array contract
 *   - ListFlags pagination envelope
 *   - EvaluateFlag response contract
 *   - Error format for not_found
 */
import { describe, it, expect } from 'vitest';
import { http, HttpResponse } from 'msw';
import { server } from '@/__mocks__/server';

const FLAGS_SVC = '*/experimentation.flags.v1.FeatureFlagService';

/** Strip proto enum prefix (mirrors stripEnumPrefix in api.ts). */
function stripFlagTypePrefix(value: string): string {
  return value.startsWith('FLAG_TYPE_') ? value.slice('FLAG_TYPE_'.length) : value;
}

/** Adapt a raw proto Flag JSON to UI-friendly shape. */
function adaptFlag(proto: Record<string, unknown>) {
  return {
    flagId: (proto.flagId as string) || '',
    name: (proto.name as string) || '',
    description: (proto.description as string) || '',
    type: stripFlagTypePrefix((proto.type as string) || 'UNSPECIFIED'),
    defaultValue: (proto.defaultValue as string) || '',
    enabled: (proto.enabled as boolean) || false,
    rolloutPercentage: (proto.rolloutPercentage as number) || 0,
    variants: ((proto.variants as Record<string, unknown>[]) || []).map((v) => ({
      variantId: (v.variantId as string) || '',
      value: (v.value as string) || '',
      trafficFraction: (v.trafficFraction as number) || 0,
    })),
    targetingRuleId: proto.targetingRuleId as string | undefined,
  };
}

describe('M6↔M7 Feature Flag wire format', () => {
  it('strips FLAG_TYPE_ prefix from all enum values', async () => {
    server.use(
      http.post(`${FLAGS_SVC}/ListFlags`, () =>
        HttpResponse.json({
          flags: [
            { flagId: 'f1', name: 'Bool Flag', type: 'FLAG_TYPE_BOOLEAN', defaultValue: 'true', enabled: true },
            { flagId: 'f2', name: 'String Flag', type: 'FLAG_TYPE_STRING', defaultValue: 'blue' },
            { flagId: 'f3', name: 'Numeric Flag', type: 'FLAG_TYPE_NUMERIC', defaultValue: '42' },
            { flagId: 'f4', name: 'JSON Flag', type: 'FLAG_TYPE_JSON', defaultValue: '{"key":"val"}' },
          ],
          nextPageToken: '',
        }),
      ),
    );

    const res = await fetch(`http://localhost/experimentation.flags.v1.FeatureFlagService/ListFlags`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ pageSize: 10 }),
    });
    const raw = await res.json() as { flags: Record<string, unknown>[] };
    const flags = raw.flags.map(adaptFlag);

    expect(flags.map((f) => f.type)).toEqual(['BOOLEAN', 'STRING', 'NUMERIC', 'JSON']);
  });

  it('handles proto3 zero-value omission — disabled flag fields', async () => {
    server.use(
      http.post(`${FLAGS_SVC}/GetFlag`, () =>
        HttpResponse.json({
          flagId: 'zero-vals',
          name: 'Disabled Flag',
          type: 'FLAG_TYPE_BOOLEAN',
          defaultValue: 'false',
          // Proto3: enabled=false, rolloutPercentage=0.0 are OMITTED
        }),
      ),
    );

    const res = await fetch(`http://localhost/experimentation.flags.v1.FeatureFlagService/GetFlag`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ flagId: 'zero-vals' }),
    });
    const raw = await res.json() as Record<string, unknown>;
    const flag = adaptFlag(raw);

    expect(flag.enabled).toBe(false);
    expect(flag.rolloutPercentage).toBe(0);
    expect(flag.description).toBe('');
    expect(flag.variants).toEqual([]);
  });

  it('preserves true boolean and non-zero rollout when present', async () => {
    server.use(
      http.post(`${FLAGS_SVC}/GetFlag`, () =>
        HttpResponse.json({
          flagId: 'active',
          name: 'Active Rollout',
          type: 'FLAG_TYPE_BOOLEAN',
          defaultValue: 'false',
          enabled: true,
          rolloutPercentage: 0.5,
        }),
      ),
    );

    const res = await fetch(`http://localhost/experimentation.flags.v1.FeatureFlagService/GetFlag`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ flagId: 'active' }),
    });
    const raw = await res.json() as Record<string, unknown>;
    const flag = adaptFlag(raw);

    expect(flag.enabled).toBe(true);
    expect(flag.rolloutPercentage).toBe(0.5);
  });

  it('maps all Flag proto fields including variants', async () => {
    server.use(
      http.post(`${FLAGS_SVC}/GetFlag`, () =>
        HttpResponse.json({
          flagId: 'full-flag',
          name: 'Full Flag',
          description: 'All fields populated',
          type: 'FLAG_TYPE_STRING',
          defaultValue: 'control',
          enabled: true,
          rolloutPercentage: 1.0,
          variants: [
            { variantId: 'v1', value: 'control', trafficFraction: 0.5 },
            { variantId: 'v2', value: 'treatment', trafficFraction: 0.5 },
          ],
          targetingRuleId: 'rule-123',
        }),
      ),
    );

    const res = await fetch(`http://localhost/experimentation.flags.v1.FeatureFlagService/GetFlag`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ flagId: 'full-flag' }),
    });
    const raw = await res.json() as Record<string, unknown>;
    const flag = adaptFlag(raw);

    expect(flag).toEqual({
      flagId: 'full-flag',
      name: 'Full Flag',
      description: 'All fields populated',
      type: 'STRING',
      defaultValue: 'control',
      enabled: true,
      rolloutPercentage: 1.0,
      variants: [
        { variantId: 'v1', value: 'control', trafficFraction: 0.5 },
        { variantId: 'v2', value: 'treatment', trafficFraction: 0.5 },
      ],
      targetingRuleId: 'rule-123',
    });
  });

  it('ListFlags pagination: passes pageSize/pageToken, reads nextPageToken', async () => {
    server.use(
      http.post(`${FLAGS_SVC}/ListFlags`, async ({ request }) => {
        const body = await request.json() as Record<string, unknown>;
        expect(body.pageSize).toBe(2);
        expect(body.pageToken).toBe('abc');
        return HttpResponse.json({
          flags: [
            { flagId: 'page1', name: 'Page 1', type: 'FLAG_TYPE_BOOLEAN', defaultValue: 'true' },
          ],
          nextPageToken: 'def',
        });
      }),
    );

    const res = await fetch(`http://localhost/experimentation.flags.v1.FeatureFlagService/ListFlags`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ pageSize: 2, pageToken: 'abc' }),
    });
    const raw = await res.json() as { flags: Record<string, unknown>[]; nextPageToken: string };

    expect(raw.flags).toHaveLength(1);
    expect(raw.nextPageToken).toBe('def');
  });

  it('EvaluateFlag response contract — flagId, value, variantId', async () => {
    server.use(
      http.post(`${FLAGS_SVC}/EvaluateFlag`, () =>
        HttpResponse.json({
          flagId: 'eval-test',
          value: 'treatment-blue',
          variantId: 'v2',
        }),
      ),
    );

    const res = await fetch(`http://localhost/experimentation.flags.v1.FeatureFlagService/EvaluateFlag`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ flagId: 'eval-test', userId: 'user-123', attributes: { country: 'US' } }),
    });
    const raw = await res.json() as Record<string, unknown>;

    expect(raw.flagId).toBe('eval-test');
    expect(raw.value).toBe('treatment-blue');
    expect(raw.variantId).toBe('v2');
  });

  it('EvaluateFlags bulk response — evaluations array', async () => {
    server.use(
      http.post(`${FLAGS_SVC}/EvaluateFlags`, () =>
        HttpResponse.json({
          evaluations: [
            { flagId: 'f1', value: 'true', variantId: 'v1' },
            { flagId: 'f2', value: 'blue', variantId: 'v3' },
          ],
        }),
      ),
    );

    const res = await fetch(`http://localhost/experimentation.flags.v1.FeatureFlagService/EvaluateFlags`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ userId: 'user-456', attributes: {} }),
    });
    const raw = await res.json() as { evaluations: Record<string, unknown>[] };

    expect(raw.evaluations).toHaveLength(2);
    expect(raw.evaluations[0].flagId).toBe('f1');
    expect(raw.evaluations[1].value).toBe('blue');
  });

  it('error format — not_found returns ConnectRPC error envelope', async () => {
    server.use(
      http.post(`${FLAGS_SVC}/GetFlag`, () =>
        HttpResponse.json(
          { code: 'not_found', message: 'Flag nonexistent not found' },
          { status: 404 },
        ),
      ),
    );

    const res = await fetch(`http://localhost/experimentation.flags.v1.FeatureFlagService/GetFlag`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ flagId: 'nonexistent' }),
    });

    expect(res.status).toBe(404);
    const error = await res.json() as Record<string, unknown>;
    expect(error.code).toBe('not_found');
    expect(error.message).toContain('not found');
  });

  it('FLAG_TYPE_UNSPECIFIED passes through as "UNSPECIFIED"', async () => {
    server.use(
      http.post(`${FLAGS_SVC}/GetFlag`, () =>
        HttpResponse.json({
          flagId: 'unspec',
          name: 'Unspecified',
          type: 'FLAG_TYPE_UNSPECIFIED',
          defaultValue: '',
        }),
      ),
    );

    const res = await fetch(`http://localhost/experimentation.flags.v1.FeatureFlagService/GetFlag`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ flagId: 'unspec' }),
    });
    const raw = await res.json() as Record<string, unknown>;
    const flag = adaptFlag(raw);

    expect(flag.type).toBe('UNSPECIFIED');
  });

  it('empty ListFlags returns empty array (proto3 empty repeated = absent)', async () => {
    server.use(
      http.post(`${FLAGS_SVC}/ListFlags`, () =>
        HttpResponse.json({}),
      ),
    );

    const res = await fetch(`http://localhost/experimentation.flags.v1.FeatureFlagService/ListFlags`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({}),
    });
    const raw = await res.json() as { flags?: Record<string, unknown>[]; nextPageToken?: string };

    expect(raw.flags || []).toEqual([]);
    expect(raw.nextPageToken || '').toBe('');
  });

  it('PromoteToExperiment returns Experiment wire format', async () => {
    server.use(
      http.post(`${FLAGS_SVC}/PromoteToExperiment`, () =>
        HttpResponse.json({
          experimentId: 'exp-promoted',
          name: 'Promoted Flag',
          type: 'EXPERIMENT_TYPE_AB',
          state: 'EXPERIMENT_STATE_DRAFT',
          variants: [
            { variantId: 'v1', name: 'control', trafficFraction: 0.5, isControl: true },
            { variantId: 'v2', name: 'treatment', trafficFraction: 0.5 },
          ],
          createdAt: '2026-03-13T00:00:00Z',
        }),
      ),
    );

    const res = await fetch(`http://localhost/experimentation.flags.v1.FeatureFlagService/PromoteToExperiment`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        flagId: 'f1',
        experimentType: 'EXPERIMENT_TYPE_AB',
        primaryMetricId: 'watch_time',
      }),
    });
    const raw = await res.json() as Record<string, unknown>;

    // PromoteToExperiment returns an Experiment proto — M6 needs to adapt it
    expect(raw.experimentId).toBe('exp-promoted');
    expect(raw.type).toBe('EXPERIMENT_TYPE_AB');
    expect(raw.state).toBe('EXPERIMENT_STATE_DRAFT');
    expect((raw.variants as unknown[]).length).toBe(2);
  });
});
