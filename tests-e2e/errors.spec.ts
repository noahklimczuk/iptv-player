/**
 * A failure has to reach the person who caused it (README §17).
 *
 * Twenty-three of the app's twenty-seven `invoke` calls were `void invoke(...)` with
 * no `.catch`, so a command that refused produced an unhandled promise rejection and
 * nothing else. Inside WebView2 there is no console to read that in: tuning a channel
 * whose every source was dead showed the banner, opened the player, and never said
 * why the picture was black.
 */
import { expect, test, type Page } from '@playwright/test';

const SHOTS = 'screenshots';

/** Make the next call to `command` refuse, the way the host refuses. */
async function failNext(page: Page, command: string, message: string) {
  await page.evaluate(
    ([c, m]) => {
      const w = window as unknown as {
        __auroraFailNext?: (name: string, msg: string) => void;
      };
      w.__auroraFailNext!(c as string, m as string);
    },
    [command, message],
  );
}

test('a channel that will not tune says so instead of showing a black player', async ({
  page,
}) => {
  await page.goto('/#/live');
  await expect(page.getByRole('heading', { name: 'Live TV' })).toBeVisible();

  await failNext(page, 'player.play', 'nothing would play on channel 1');
  await page.getByTestId('channel-row').first().click();

  const notices = page.getByTestId('notices');
  await expect(notices).toBeVisible();
  await expect(notices).toContainText('Could not tune');
  await expect(notices).toContainText('nothing would play on channel 1');

  await page.screenshot({ path: `${SHOTS}/40-tune-failure.png` });
});

test('a notice can be dismissed', async ({ page }) => {
  await page.goto('/#/live');
  await failNext(page, 'player.play', 'nothing would play on channel 1');
  await page.getByTestId('channel-row').first().click();

  const notices = page.getByTestId('notices');
  await expect(notices).toBeVisible();
  await notices.getByRole('button', { name: 'Dismiss' }).first().click();
  await expect(notices).toHaveCount(0);
});

/**
 * Zapping quickly is normal, and the host answers the tune you replaced with
 * `AppError::Superseded`. A toast for every double press of Ch+ would be worse than
 * the silence this replaced.
 */
test('a tune the viewer replaced is not reported as a failure', async ({ page }) => {
  await page.goto('/#/live');
  await expect(page.getByRole('heading', { name: 'Live TV' })).toBeVisible();

  await failNext(page, 'player.play', 'superseded by a newer request');
  await page.getByTestId('channel-row').first().click();

  await page.waitForTimeout(400);
  await expect(page.getByTestId('notices')).toHaveCount(0);
});
