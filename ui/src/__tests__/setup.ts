import '@testing-library/jest-dom/vitest';
import { server } from '@/__mocks__/server';
import { resetSeedData } from '@/__mocks__/seed-data';
import { beforeAll, afterEach, afterAll } from 'vitest';

beforeAll(() => server.listen({ onUnhandledRequest: 'error' }));
afterEach(() => {
  server.resetHandlers();
  resetSeedData();
});
afterAll(() => server.close());
