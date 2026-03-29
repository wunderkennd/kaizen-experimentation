import '@testing-library/jest-dom/vitest';
import { server } from '@/__mocks__/server';
import { resetSeedData } from '@/__mocks__/seed-data';
import { clearApiCache } from '@/lib/api';
import { beforeAll, afterEach, afterAll } from 'vitest';

beforeAll(() => server.listen({ onUnhandledRequest: 'error' }));
afterEach(() => {
  server.resetHandlers();
  resetSeedData();
  clearApiCache();
});
afterAll(() => server.close());

// Mock clipboard
if (typeof window !== 'undefined') {
  Object.defineProperty(navigator, 'clipboard', {
    value: {
      writeText: vi.fn().mockImplementation(() => Promise.resolve()),
    },
    configurable: true,
  });
}
