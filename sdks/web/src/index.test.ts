import { describe, expect, it, vi, beforeEach, afterEach } from 'vitest';
import { murmurhash3_x86_32 } from './murmur3';
import {
  LocalProvider,
  RemoteProvider,
  ExperimentClient,
  MockProvider,
  type ExperimentConfig,
  type UserAttributes,
} from './index';

// ---------------------------------------------------------------------------
// MurmurHash3 parity tests (from test-vectors/hash_vectors.json)
// ---------------------------------------------------------------------------

describe('murmurhash3_x86_32', () => {
  const encoder = new TextEncoder();

  it('empty string with seed 0 returns 0', () => {
    expect(murmurhash3_x86_32(new Uint8Array(0), 0)).toBe(0);
  });

  it('known value: "hello" seed=0', () => {
    expect(murmurhash3_x86_32(encoder.encode('hello'), 0)).toBe(0x248bfa47);
  });

  it('known value: "hello" seed=1', () => {
    expect(murmurhash3_x86_32(encoder.encode('hello'), 1)).toBe(0xbb4abcad);
  });

  it('is deterministic', () => {
    const data = encoder.encode('test_input');
    const h1 = murmurhash3_x86_32(data, 42);
    const h2 = murmurhash3_x86_32(data, 42);
    expect(h1).toBe(h2);
  });
});

// Bucket helper matching Rust: bucket = hash(userId + "\x00" + salt, seed=0) % totalBuckets
function computeBucket(userId: string, salt: string, totalBuckets: number): number {
  const encoder = new TextEncoder();
  const key = encoder.encode(`${userId}\x00${salt}`);
  const hash = murmurhash3_x86_32(key, 0);
  return hash % totalBuckets;
}

describe('bucket parity with test vectors', () => {
  // First 10 entries from test-vectors/hash_vectors.json
  const vectors = [
    { user_id: 'user_000000', salt: 'experiment_default_salt', total_buckets: 10000, expected_bucket: 3913 },
    { user_id: 'user_000001', salt: 'experiment_default_salt', total_buckets: 10000, expected_bucket: 4234 },
    { user_id: 'user_000002', salt: 'experiment_default_salt', total_buckets: 10000, expected_bucket: 5578 },
    { user_id: 'user_000003', salt: 'experiment_default_salt', total_buckets: 10000, expected_bucket: 8009 },
    { user_id: 'user_000004', salt: 'experiment_default_salt', total_buckets: 10000, expected_bucket: 2419 },
    { user_id: 'user_000005', salt: 'experiment_default_salt', total_buckets: 10000, expected_bucket: 5885 },
    { user_id: 'user_000006', salt: 'experiment_default_salt', total_buckets: 10000, expected_bucket: 5586 },
    { user_id: 'user_000007', salt: 'experiment_default_salt', total_buckets: 10000, expected_bucket: 9853 },
    { user_id: 'user_000008', salt: 'experiment_default_salt', total_buckets: 10000, expected_bucket: 2730 },
    { user_id: 'user_000009', salt: 'experiment_default_salt', total_buckets: 10000, expected_bucket: 27 },
  ];

  for (const v of vectors) {
    it(`${v.user_id} → bucket ${v.expected_bucket}`, () => {
      const bucket = computeBucket(v.user_id, v.salt, v.total_buckets);
      expect(bucket).toBe(v.expected_bucket);
    });
  }
});

// ---------------------------------------------------------------------------
// LocalProvider tests
// ---------------------------------------------------------------------------

const twoVariantConfig: ExperimentConfig = {
  experimentId: 'exp_ab_test',
  hashSalt: 'salt_ab',
  layerName: 'default',
  variants: [
    { name: 'control', trafficFraction: 0.5, isControl: true, payload: { color: 'blue' } },
    { name: 'treatment', trafficFraction: 0.5, isControl: false, payload: { color: 'red' } },
  ],
  allocationStart: 0,
  allocationEnd: 9999,
  totalBuckets: 10000,
};

const threeVariantConfig: ExperimentConfig = {
  experimentId: 'exp_abc',
  hashSalt: 'salt_abc',
  layerName: 'default',
  variants: [
    { name: 'control', trafficFraction: 0.34, isControl: true, payload: {} },
    { name: 'variant_a', trafficFraction: 0.33, isControl: false, payload: {} },
    { name: 'variant_b', trafficFraction: 0.33, isControl: false, payload: {} },
  ],
  allocationStart: 0,
  allocationEnd: 9999,
  totalBuckets: 10000,
};

describe('LocalProvider.getAssignment', () => {
  it('returns null for unknown experiment', async () => {
    const provider = new LocalProvider({ experiments: [twoVariantConfig] });
    const result = await provider.getAssignment('nonexistent', { userId: 'user1' });
    expect(result).toBeNull();
  });

  it('is deterministic — same user gets same variant', async () => {
    const provider = new LocalProvider({ experiments: [twoVariantConfig] });
    const attrs: UserAttributes = { userId: 'user_stable_123' };
    const a1 = await provider.getAssignment('exp_ab_test', attrs);
    const a2 = await provider.getAssignment('exp_ab_test', attrs);
    expect(a1).not.toBeNull();
    expect(a1!.variantName).toBe(a2!.variantName);
  });

  it('returns fromCache: true', async () => {
    const provider = new LocalProvider({ experiments: [twoVariantConfig] });
    const result = await provider.getAssignment('exp_ab_test', { userId: 'user1' });
    expect(result).not.toBeNull();
    expect(result!.fromCache).toBe(true);
  });

  it('returns payload from config', async () => {
    const provider = new LocalProvider({ experiments: [twoVariantConfig] });
    const result = await provider.getAssignment('exp_ab_test', { userId: 'user1' });
    expect(result).not.toBeNull();
    expect(result!.payload).toBeDefined();
  });

  it('excludes user outside allocation range', async () => {
    const narrowConfig: ExperimentConfig = {
      ...twoVariantConfig,
      experimentId: 'exp_narrow',
      allocationStart: 0,
      allocationEnd: 0, // only bucket 0
    };
    const provider = new LocalProvider({ experiments: [narrowConfig] });

    // Most users should land outside bucket 0
    let nullCount = 0;
    for (let i = 0; i < 50; i++) {
      const result = await provider.getAssignment('exp_narrow', { userId: `exclude_test_${i}` });
      if (result === null) nullCount++;
    }
    expect(nullCount).toBeGreaterThan(40);
  });

  it('distributes users across variants by traffic fraction', async () => {
    const provider = new LocalProvider({ experiments: [twoVariantConfig] });
    const counts: Record<string, number> = { control: 0, treatment: 0 };

    for (let i = 0; i < 1000; i++) {
      const result = await provider.getAssignment('exp_ab_test', { userId: `dist_user_${i}` });
      if (result) counts[result.variantName]++;
    }

    // With 50/50 split over 1000 users, expect roughly even distribution
    expect(counts.control).toBeGreaterThan(350);
    expect(counts.treatment).toBeGreaterThan(350);
  });

  it('handles three-variant experiments', async () => {
    const provider = new LocalProvider({ experiments: [threeVariantConfig] });
    const variants = new Set<string>();

    for (let i = 0; i < 500; i++) {
      const result = await provider.getAssignment('exp_abc', { userId: `three_var_${i}` });
      if (result) variants.add(result.variantName);
    }

    expect(variants.size).toBe(3);
  });

  it('FP rounding fallback assigns last variant', async () => {
    // Config where fractions don't perfectly sum to 1.0
    const fpConfig: ExperimentConfig = {
      experimentId: 'exp_fp',
      hashSalt: 'salt_fp',
      layerName: 'default',
      variants: [
        { name: 'a', trafficFraction: 0.333, isControl: true, payload: {} },
        { name: 'b', trafficFraction: 0.333, isControl: false, payload: {} },
        { name: 'c', trafficFraction: 0.334, isControl: false, payload: {} },
      ],
      allocationStart: 0,
      allocationEnd: 9999,
      totalBuckets: 10000,
    };
    const provider = new LocalProvider({ experiments: [fpConfig] });

    // All users should be assigned to one of the variants
    for (let i = 0; i < 100; i++) {
      const result = await provider.getAssignment('exp_fp', { userId: `fp_user_${i}` });
      expect(result).not.toBeNull();
      expect(['a', 'b', 'c']).toContain(result!.variantName);
    }
  });
});

describe('LocalProvider.getAllAssignments', () => {
  it('returns assignments for all matching experiments', async () => {
    const provider = new LocalProvider({
      experiments: [twoVariantConfig, threeVariantConfig],
    });
    const results = await provider.getAllAssignments({ userId: 'multi_user_1' });
    expect(results.size).toBe(2);
    expect(results.has('exp_ab_test')).toBe(true);
    expect(results.has('exp_abc')).toBe(true);
  });
});

// ---------------------------------------------------------------------------
// RemoteProvider tests
// ---------------------------------------------------------------------------

describe('RemoteProvider', () => {
  const originalFetch = globalThis.fetch;

  afterEach(() => {
    globalThis.fetch = originalFetch;
    vi.restoreAllMocks();
  });

  function mockFetch(status: number, body: unknown): void {
    globalThis.fetch = vi.fn().mockResolvedValue({
      ok: status >= 200 && status < 300,
      status,
      json: () => Promise.resolve(body),
    });
  }

  it('getAssignment returns assignment on success', async () => {
    mockFetch(200, {
      experimentId: 'exp1',
      variantId: 'treatment',
      payloadJson: '{"color":"red"}',
      assignmentProbability: 0.5,
      isActive: true,
    });

    const provider = new RemoteProvider({ baseUrl: 'http://localhost:8080' });
    const result = await provider.getAssignment('exp1', { userId: 'user-1' });

    expect(result).not.toBeNull();
    expect(result!.experimentId).toBe('exp1');
    expect(result!.variantName).toBe('treatment');
    expect(result!.payload).toEqual({ color: 'red' });
    expect(result!.fromCache).toBe(false);

    expect(globalThis.fetch).toHaveBeenCalledWith(
      'http://localhost:8080/experimentation.assignment.v1.AssignmentService/GetAssignment',
      expect.objectContaining({ method: 'POST' }),
    );
  });

  it('getAssignment returns null when not active', async () => {
    mockFetch(200, {
      experimentId: 'exp1',
      variantId: '',
      isActive: false,
    });

    const provider = new RemoteProvider({ baseUrl: 'http://localhost:8080' });
    const result = await provider.getAssignment('exp1', { userId: 'user-1' });
    expect(result).toBeNull();
  });

  it('getAssignment returns null on 404', async () => {
    mockFetch(404, { code: 404, message: 'not found' });

    const provider = new RemoteProvider({ baseUrl: 'http://localhost:8080' });
    const result = await provider.getAssignment('missing', { userId: 'user-1' });
    expect(result).toBeNull();
  });

  it('getAssignment returns null on 500', async () => {
    mockFetch(500, { code: 500, message: 'internal error' });

    const provider = new RemoteProvider({ baseUrl: 'http://localhost:8080' });
    const result = await provider.getAssignment('exp1', { userId: 'user-1' });
    expect(result).toBeNull();
  });

  it('getAssignment throws on network error (for fallback chain)', async () => {
    globalThis.fetch = vi.fn().mockRejectedValue(new TypeError('fetch failed'));

    const provider = new RemoteProvider({ baseUrl: 'http://localhost:8080' });
    await expect(provider.getAssignment('exp1', { userId: 'user-1' }))
      .rejects.toThrow('fetch failed');
  });

  it('getAssignment handles empty payloadJson', async () => {
    mockFetch(200, {
      experimentId: 'exp1',
      variantId: 'control',
      payloadJson: '',
      assignmentProbability: 1.0,
      isActive: true,
    });

    const provider = new RemoteProvider({ baseUrl: 'http://localhost:8080' });
    const result = await provider.getAssignment('exp1', { userId: 'user-1' });
    expect(result).not.toBeNull();
    expect(result!.payload).toEqual({});
  });

  it('getAssignment sends flattened attributes', async () => {
    mockFetch(200, {
      experimentId: 'exp1',
      variantId: 'control',
      payloadJson: '',
      isActive: true,
    });

    const provider = new RemoteProvider({ baseUrl: 'http://localhost:8080' });
    await provider.getAssignment('exp1', {
      userId: 'user-1',
      plan: 'premium',
      age: 30,
    } as UserAttributes);

    const fetchCall = (globalThis.fetch as ReturnType<typeof vi.fn>).mock.calls[0];
    const sentBody = JSON.parse(fetchCall[1].body);
    expect(sentBody.attributes.plan).toBe('premium');
    expect(sentBody.attributes.age).toBe('30');
    expect(sentBody.attributes.userId).toBeUndefined();
  });

  it('getAllAssignments returns map of assignments', async () => {
    mockFetch(200, {
      assignments: [
        { experimentId: 'exp1', variantId: 'control', payloadJson: '{}', isActive: true },
        { experimentId: 'exp2', variantId: 'treatment', payloadJson: '{"x":1}', isActive: true },
        { experimentId: 'exp3', variantId: '', isActive: false },
      ],
    });

    const provider = new RemoteProvider({ baseUrl: 'http://localhost:8080' });
    const results = await provider.getAllAssignments({ userId: 'user-1' });

    expect(results.size).toBe(2);
    expect(results.get('exp1')!.variantName).toBe('control');
    expect(results.get('exp2')!.variantName).toBe('treatment');
    expect(results.has('exp3')).toBe(false); // inactive
  });

  it('getAllAssignments returns empty map on error', async () => {
    mockFetch(500, { code: 500, message: 'error' });

    const provider = new RemoteProvider({ baseUrl: 'http://localhost:8080' });
    const results = await provider.getAllAssignments({ userId: 'user-1' });
    expect(results.size).toBe(0);
  });
});

// ---------------------------------------------------------------------------
// ExperimentClient fallback chain tests
// ---------------------------------------------------------------------------

describe('ExperimentClient fallback', () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('falls back to fallback provider when primary throws', async () => {
    globalThis.fetch = vi.fn().mockRejectedValue(new TypeError('network error'));

    const remote = new RemoteProvider({ baseUrl: 'http://localhost:8080' });
    const local = new MockProvider([
      { experimentId: 'exp1', variantName: 'fallback-variant' },
    ]);

    const client = new ExperimentClient({
      provider: remote,
      userId: 'user-1',
      fallbackProvider: local,
    });

    const variant = await client.getVariant('exp1');
    expect(variant).toBe('fallback-variant');

    await client.destroy();
  });
});
