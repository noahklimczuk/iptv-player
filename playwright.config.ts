import { defineConfig } from '@playwright/test';

export default defineConfig({
  testDir: './tests-e2e',
  timeout: 45_000,
  fullyParallel: false,
  reporter: [['list']],
  use: {
    baseURL: 'http://127.0.0.1:4173',
    viewport: { width: 1600, height: 950 },
    deviceScaleFactor: 1,
    launchOptions: { executablePath: '/opt/pw-browsers/chromium-1194/chrome-linux/chrome' },
  },
  webServer: {
    command: 'npx vite preview --port 4173 --strictPort',
    port: 4173,
    reuseExistingServer: true,
    timeout: 60_000,
  },
});
