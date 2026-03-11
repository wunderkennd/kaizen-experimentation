import { describe, it, expect, vi, beforeEach } from 'vitest';
import { listExperiments, getExperiment, clearApiCache } from '@/lib/api';

describe('API request cache', () => {
  beforeEach(() => {
    clearApiCache();
  });

  it('returns cached response on second call within TTL', async () => {
    const fetchSpy = vi.spyOn(globalThis, 'fetch');

    const first = await listExperiments();
    const second = await listExperiments();

    // fetch should only be called once — second call served from cache
    expect(fetchSpy.mock.calls.filter(c =>
      (c[0] as string).includes('ListExperiments'),
    )).toHaveLength(1);

    expect(first.experiments).toEqual(second.experiments);
    fetchSpy.mockRestore();
  });

  it('caches separately for different experiment IDs', async () => {
    const fetchSpy = vi.spyOn(globalThis, 'fetch');

    await getExperiment('11111111-1111-1111-1111-111111111111');
    await getExperiment('33333333-3333-3333-3333-333333333333');

    // Two different IDs = two fetch calls
    expect(fetchSpy.mock.calls.filter(c =>
      (c[0] as string).includes('GetExperiment'),
    )).toHaveLength(2);

    fetchSpy.mockRestore();
  });

  it('clearApiCache forces re-fetch on next call', async () => {
    const fetchSpy = vi.spyOn(globalThis, 'fetch');

    await listExperiments();
    clearApiCache();
    await listExperiments();

    // After clear, second call should fetch again
    expect(fetchSpy.mock.calls.filter(c =>
      (c[0] as string).includes('ListExperiments'),
    )).toHaveLength(2);

    fetchSpy.mockRestore();
  });
});
