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
     * Bind IPv4 explicitly and wait on that same address. `vite preview` otherwise
     * binds whatever `localhost` resolves to; on a host where that is `::1` it
     * listens on IPv6 only, Playwright's port probe succeeds, and every test then
     * gets ECONNREFUSED against 127.0.0.1.
     */
    command: `pnpm exec vite preview --port ${PORT} --strictPort --host 127.0.0.1`,
    url: ORIGIN,
    reuseExistingServer: !process.env.CI,
    timeout: 60_000,
    stdout: 'pipe',
    stderr: 'pipe',
  },
});
