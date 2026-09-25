import { defineConfig } from 'vitest/config';
import { fileURLToPath, URL } from 'node:url';

/**
 * `pnpm test` used to answer "No test files found, exiting with code 1", because
 * there were none: not for `shared/ipc.ts`, not for the formatters, not for the
 * zapper's arithmetic. The 84 Playwright journeys all run against the *mock*
 * transport, which is exactly why a host that never sent `Channel.favorite` and a CSP
 * that blocked every provider logo were both invisible from here.
 *
 * Node environment rather than jsdom: nothing under test needs a DOM, and the two
 * places that touch `window` are small enough to stub. Adding jsdom to run four
 * listeners would be a dependency for its own sake.
 */
export default defineConfig({
  resolve: {
    alias: {
      '@': fileURLToPath(new URL('./src-ui/src', import.meta.url)),
      '@shared': fileURLToPath(new URL('./shared', import.meta.url)),
    },
  },
  test: {
    environment: 'node',
    include: ['src-ui/src/**/*.test.ts', 'shared/**/*.test.ts'],
    // Playwright specs live in tests-e2e and are driven by `pnpm shots`; picking them
    // up here would start a browser inside a unit-test run.
    exclude: ['**/node_modules/**', 'tests-e2e/**', 'dist/**'],
  },
});
