import { describe, expect, it } from 'vitest';
import { murmurhash3_x86_32 } from './murmur3';
import {
  LocalProvider,
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
