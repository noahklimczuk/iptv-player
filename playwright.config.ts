import { existsSync } from 'node:fs';
import { defineConfig } from '@playwright/test';

/**
 * Prefer a Chromium that is already on the machine (this repo's dev container ships
 * one at PLAYWRIGHT_BROWSERS_PATH), and otherwise fall back to the browser Playwright
 * manages itself — which is what CI runners have. Hardcoding a path breaks one or the
 * other, so probe for it.
 */
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
    baseURL: 'http://127.0.0.1:4173',
    viewport: { width: 1600, height: 950 },
    deviceScaleFactor: 1,
    ...(existsSync(preinstalled) ? { launchOptions: { executablePath: preinstalled } } : {}),
  },
  webServer: {
    command: 'npx vite preview --port 4173 --strictPort',
    port: 4173,
    reuseExistingServer: !process.env.CI,
    timeout: 60_000,
  },
});
