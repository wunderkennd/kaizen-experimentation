import { defineConfig } from 'vitest/config';
import path from 'path';

export default defineConfig({
  esbuild: {
    jsx: 'automatic',
  },
  test: {
    environment: 'jsdom',
    setupFiles: ['./src/__tests__/setup.ts'],
    globals: true,
    env: {
      NEXT_PUBLIC_MANAGEMENT_URL: 'http://localhost:50055',
      NEXT_PUBLIC_METRICS_URL: 'http://localhost:50056',
      NEXT_PUBLIC_ANALYSIS_URL: 'http://localhost:50053',
      NEXT_PUBLIC_BANDIT_URL: 'http://localhost:50054',
      NEXT_PUBLIC_USER_ROLE: 'experimenter',
      NEXT_PUBLIC_USER_EMAIL: 'test@streamco.com',
    },
  },
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
});
