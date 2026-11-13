import { defineConfig } from '@playwright/test';

// The desktop app end to end (`e2e/app.spec.ts`): one build of the app, driven through its own
// window. One test at a time: they share the app and its task list.
export default defineConfig({
  testDir: 'e2e',
  timeout: 90_000,
  workers: 1,
  reporter: [['list']],
});
