import { defineConfig } from '@playwright/test';

// The extension in Chromium (`pnpm e2e` builds it first). One browser at a time: the tests share
// a persistent profile with the extension loaded.
export default defineConfig({
  testDir: 'e2e',
  timeout: 60_000,
  workers: 1,
  reporter: [['list']],
});
