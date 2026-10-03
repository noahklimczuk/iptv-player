import { existsSync } from 'node:fs';
import { defineConfig } from '@playwright/test';

/**
 * Prefer a Chromium that is already on the machine (this repo's dev container ships
 * one at PLAYWRIGHT_BROWSERS_PATH), and otherwise fall back to the browser Playwright
 * manages itself — which is what CI runners have. Hardcoding a path breaks one or the
 * other, so probe for it.
 */
/** Overridable so a stale server on the default port cannot block a run. */
const PORT = Number(process.env.PW_PORT ?? 4173);
const ORIGIN = `http://127.0.0.1:${PORT}`;

const preinstalled =
  process.env.PLAYWRIGHT_CHROMIUM_PATH ??
  '/opt/pw-browsers/chromium-1194/chrome-linux/chrome';

export default defineConfig({
  testDir: './tests-e2e',
  // `pnpm typecheck` emits a .js beside every spec. Without this, Playwright collects
  // both and every journey runs twice — and a stale .js keeps running after its source
  // has changed.
  testMatch: /.*\.spec\.ts$/,
  timeout: 45_000,
  fullyParallel: false,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 1 : 0,
  reporter: [['list']],
  use: {
    baseURL: ORIGIN,
    viewport: { width: 1600, height: 950 },
    deviceScaleFactor: 1,
    ...(existsSync(preinstalled) ? { launchOptions: { executablePath: preinstalled } } : {}),
  },
  webServer: {
    /**
     * Build, then serve what was built.
     *
     * `vite preview` serves `dist/`, and nothing here used to put anything in it. CI has
     * a build step before this and a local run did not, so running the journeys after
     * editing the UI tested whatever was last built — which on this machine was a bundle
     * from the previous week. Every one of them passed, against code that no longer
     * existed. The build is a couple of seconds and it is the difference between a green
     * suite and a meaningful one.
     *
     * Bind IPv4 explicitly and wait on that same address. `vite preview` otherwise
     * binds whatever `localhost` resolves to; on a host where that is `::1` it
     * listens on IPv6 only, Playwright's port probe succeeds, and every test then
     * gets ECONNREFUSED against 127.0.0.1.
     */
    command:
      `pnpm exec vite build && `
      + `pnpm exec vite preview --port ${PORT} --strictPort --host 127.0.0.1`,
    url: ORIGIN,
    /**
     * Never locally, despite the cost of a rebuild each run.
     *
     * Reusing a server that is already up skips the build above with it, which is exactly
     * the trap this is meant to close: the second run of the day would go back to serving
     * whatever the first one built.
     */
    reuseExistingServer: false,
    // Longer, because it now covers the build as well as the server coming up.
    timeout: 120_000,
    stdout: 'pipe',
    stderr: 'pipe',
  },
});
