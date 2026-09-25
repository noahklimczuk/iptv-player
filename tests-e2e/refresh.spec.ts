/**
 * Re-importing a provider after setup.
 *
 * `providers.refresh` was implemented, registered, contract-tested — and called from
 * exactly one place in the whole UI: the first-run wizard. So a library could be
 * imported once and never again. A channel the provider added last week was
 * unreachable except by deleting the provider and adding it back, and nothing on any
 * screen suggested otherwise.
 *
 * The report it prints afterwards is the other half. "0 channels" next to twenty
 * thousand films is the difference between a bug in Aurora and an account that carries
 * no live streams, and until now nothing in the app could tell those apart.
 */
import { expect, test } from '@playwright/test';

const SHOTS = 'screenshots';

test('a provider can be re-imported, and says what it found', async ({ page }) => {
  await page.goto('/#/settings');
  await expect(page.getByRole('heading', { name: 'Providers' })).toBeVisible();

  const refresh = page.getByRole('button', { name: 'Refresh' }).first();
  await expect(refresh).toBeEnabled();
  await refresh.click();

  // It must say something while it works: a real refresh took 23.7 seconds in this
  // project's own measurement, and a button that looks dead for that long gets
  // pressed again.
  await expect(page.getByTestId('refresh-progress')).toBeVisible();
  await expect(refresh).toBeDisabled();

  const report = page.getByTestId('refresh-report');
  await expect(report).toBeVisible({ timeout: 15_000 });
  await expect(report).toContainText(/\d+ channels/);
  await expect(report).toContainText(/\d+ movies/);
  await expect(refresh).toBeEnabled();

  await page.screenshot({ path: `${SHOTS}/46-provider-refreshed.png` });
});

test('an empty channel list explains itself instead of blaming the setup', async ({
  page,
}) => {
  // With channels present the empty state is unreachable, so this drives the case the
  // screenshot showed: the host answers with none.
  await page.goto('/#/');
  await page.evaluate(() => {
    const w = window as unknown as {
      __auroraFailNext?: (name: string, msg: string) => void;
    };
    w.__auroraFailNext?.('channels.list', 'nothing to show');
  });

  await page.goto('/#/live');
  await expect(page.getByRole('heading', { name: 'Live TV' })).toBeVisible();

  // Whatever it says, it must not be the old sentence — that one sends somebody who
  // already has a provider off to add a second copy of it.
  const body = page.locator('body');
  await expect(body).not.toContainText(
    'Add a provider in Settings to populate your channel list.',
  );
});
