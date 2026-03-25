import '@testing-library/jest-dom/vitest';
import { server } from '@/__mocks__/server';
import { resetSeedData } from '@/__mocks__/seed-data';
import { clearApiCache } from '@/lib/api';
import { beforeAll, afterEach, afterAll, vi } from 'vitest';

// Mock navigator.clipboard
if (typeof window !== 'undefined') {
  Object.defineProperty(navigator, 'clipboard', {
    value: {
      writeText: vi.fn().mockResolvedValue(undefined),
    },
    configurable: true,
  });
}

beforeAll(() => server.listen({ onUnhandledRequest: 'error' }));
afterEach(() => {
  server.resetHandlers();
  resetSeedData();
  clearApiCache();
});
afterAll(() => server.close());
