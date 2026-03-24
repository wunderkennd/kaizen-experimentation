import '@testing-library/jest-dom/vitest';
import { server } from '@/__mocks__/server';
import { resetSeedData } from '@/__mocks__/seed-data';
import { clearApiCache } from '@/lib/api';
import { beforeAll, afterEach, afterAll, vi } from 'vitest';

beforeAll(() => {
  server.listen({ onUnhandledRequest: 'error' });
  // Mock clipboard
  vi.stubGlobal('navigator', {
    ...navigator,
    clipboard: {
      writeText: vi.fn().mockResolvedValue(undefined),
    },
  });
});
afterEach(() => {
  server.resetHandlers();
  resetSeedData();
  clearApiCache();
});
afterAll(() => server.close());
