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

/**
 * Eight seconds means eight seconds, however many notices arrive meanwhile.
 *
 * The dismiss effect depended on the whole `notices` array, so every change tore down
 * all the timers and started them again from zero: a toast that had been up for seven of
 * its eight seconds went back to eight the moment another arrived. `errors.ts` caps the
 * stack at three and dedupes by title, so this could not grow without bound — but it did
 * not bound the *time*, and a run of distinct failures arriving more often than every
 * eight seconds keeps the oldest one on screen for as long as the run lasts (R-97).
 *
 * Driven with Playwright's clock rather than by waiting, so the test is instant and the
 * eight seconds are exact.
 */
test('a second notice does not restart the first one\'s timer', async ({ page }) => {
  await page.clock.install();
  await page.goto('/#/live');
  await expect(page.getByRole('heading', { name: 'Live TV' })).toBeVisible();

  const notices = page.getByTestId('notices');

  // The first failure, with a message of its own so it can be told apart.
  await failNext(page, 'player.play', 'the first failure');
  await page.getByTestId('channel-row').first().click();
  await expect(notices).toContainText('the first failure');

  // The player opened over the list, so leave it before reaching for another row.
  await page.keyboard.press('Escape');
  await expect(page.getByTestId('channel-row').nth(1)).toBeVisible();

  // Six seconds later — three quarters of its life — a different failure arrives. A
  // different channel, so the notice has a different title: `errors.ts` dedupes by
  // title, and two notices with the same one would be one notice.
  await page.clock.fastForward(6000);
  await failNext(page, 'player.play', 'the second failure');
  await page.getByTestId('channel-row').nth(1).click();
  await expect(notices).toContainText('the second failure');

  // Three more seconds: the first notice has now existed for nine, so it must be gone.
  // Against the old code its timer was restarted at six seconds and it is still there.
  await page.clock.fastForward(3000);
  await expect(notices, 'the first notice outlived its eight seconds')
    .not.toContainText('the first failure');
  // And the second one, at three seconds old, is still up.
  await expect(notices).toContainText('the second failure');

  // Which also goes, on its own eight seconds rather than the first one's.
  await page.clock.fastForward(6000);
  await expect(notices).toHaveCount(0);
});
