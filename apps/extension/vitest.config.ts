import { defineConfig } from 'vitest/config';

// Pure logic only (rules, headers, pairing); the browser-facing paths are covered by the
// Playwright tests in e2e/.
export default defineConfig({
  test: { include: ['lib/**/*.test.ts'] },
});
