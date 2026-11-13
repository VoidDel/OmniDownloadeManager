// ESLint for the desktop frontend: JavaScript and TypeScript recommended rules plus the rules of
// hooks. Type checking itself is `tsc` (`pnpm lint`).
import js from '@eslint/js';
import { defineConfig, globalIgnores } from 'eslint/config';
import reactHooks from 'eslint-plugin-react-hooks';
import globals from 'globals';
import tseslint from 'typescript-eslint';

export default defineConfig(
  globalIgnores(['dist', 'src-tauri', 'test-results', 'playwright-report']),
  js.configs.recommended,
  tseslint.configs.recommended,
  reactHooks.configs.flat.recommended,
  {
    files: ['src/**/*.{ts,tsx}'],
    languageOptions: { globals: globals.browser },
  },
  {
    files: ['*.{js,ts}', 'e2e/**/*.ts'],
    languageOptions: { globals: globals.node },
  },
);
