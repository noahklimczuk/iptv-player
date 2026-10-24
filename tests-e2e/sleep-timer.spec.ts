/**
 * The sleep timer (README §13).
 *
 * "15/30/45/60/90 min, 'end of programme', 'end of episode', with a 60-second warning
 * and an optional PC-shutdown action."
 *
 * The 60-second warning is not driven from here. Its rule — up for the whole last
 * minute, not at one instant — is `aurora_app::sleep`'s
 * `the_warning_covers_the_last_minute`, and the screen renders `state.warning` with no
 * logic of its own, so there is nothing here that a journey could get wrong and that
 * test could not. Waiting out a real minute to watch a flag the host already set would
 * add a minute to every run and assert nothing new.
 */
import { expect, test, type Page } from '@playwright/test';

/** Open the player on a live channel, with the playback panel up. */
async function livePanel(page: Page) {
  await page.goto('/#/live');
  await expect(page.getByTestId('channel-row').first()).toBeVisible();
  await page.getByTestId('channel-row').first().click();
  await expect(page.getByRole('button', { name: 'Pause' })).toBeVisible();
  await page.mouse.move(640, 360);
  await page.getByRole('button', { name: 'Playback settings' }).click();
  const panel = page.getByTestId('playback-panel');
  await expect(panel).toBeVisible();
  return panel;
}

/** Open the player on a film, which is the thing with a runtime. */
async function filmPanel(page: Page) {
  await page.goto('/#/movies');
  await expect(page.getByTestId('catalog-card').first()).toBeVisible();
  await page.getByTestId('catalog-card').first().getByRole('button').first().click();
  await page.getByRole('dialog').getByRole('button', { name: /^(Play|Resume)/ }).click();
  await expect(page.getByRole('button', { name: 'Pause' })).toBeVisible();
  await page.mouse.move(640, 360);
  await page.getByRole('button', { name: 'Playback settings' }).click();
  const panel = page.getByTestId('playback-panel');
  await expect(panel).toBeVisible();
  return panel;
}

test('every duration README names arms the timer', async ({ page }) => {
  const panel = await livePanel(page);

  for (const minutes of [15, 30, 45, 60, 90]) {
    await expect(panel.getByRole('button', { name: `${minutes} min` })).toBeVisible();
  }

  await panel.getByRole('button', { name: '45 min' }).click();
  const armed = panel.getByTestId('sleep-armed');
  await expect(armed).toBeVisible();
  // 45 minutes, said in minutes rather than as 2700 seconds.
  await expect(armed).toHaveText(/Stopping in 45 min/);

  // The durations are gone while one is armed: the panel offers what to do about the
  // timer it has, not five ways to replace it.
  await expect(panel.getByRole('button', { name: '15 min' })).toHaveCount(0);
});

test('an armed timer can be extended and cancelled', async ({ page }) => {
  const panel = await livePanel(page);
  await panel.getByRole('button', { name: '15 min' }).click();
  await expect(panel.getByTestId('sleep-armed')).toHaveText(/15 min/);

  await panel.getByRole('button', { name: '+30 min' }).click();
  await expect(panel.getByTestId('sleep-armed')).toHaveText(/45 min/);

  await panel.getByRole('button', { name: 'Off' }).click();
  await expect(panel.getByTestId('sleep-armed')).toHaveCount(0);
  // And back to offering the durations, which is the positive half of "cancelled".
  await expect(panel.getByRole('button', { name: '15 min' })).toBeVisible();
});

test('live TV offers the end of the programme and uses the guide for it', async ({ page }) => {
  const panel = await livePanel(page);
  await expect(panel.getByRole('button', { name: 'End of film' })).toHaveCount(0);
  await panel.getByRole('button', { name: 'End of programme' }).click();

  // Armed, and at something: the guide's stop time, which is minutes away rather than
  // the flat durations above.
  const armed = panel.getByTestId('sleep-armed');
  await expect(armed).toBeVisible();
  await expect(armed).toHaveText(/Stopping in \d+/);
});

test('a film offers the end of the film and not the end of a programme', async ({ page }) => {
  const panel = await filmPanel(page);
  await expect(panel.getByRole('button', { name: 'End of programme' })).toHaveCount(0);

  const endOf = panel.getByRole('button', { name: /^End of (film|episode)$/ });
  await expect(endOf).toBeVisible();
  await endOf.click();
  await expect(panel.getByTestId('sleep-armed')).toBeVisible();
});

test('the timer survives the OSD hiding and coming back', async ({ page }) => {
  const panel = await livePanel(page);
  await panel.getByRole('button', { name: '30 min' }).click();
  await expect(panel.getByTestId('sleep-armed')).toBeVisible();

  // Let the chrome time out. The host holds the deadline, so this is not a test of a
  // React state that happens to survive — the panel re-reads it.
  await page.mouse.move(10, 10);
  await expect(page.getByRole('button', { name: 'Playback settings' })).toBeHidden({
    timeout: 10_000,
  });

  await page.mouse.move(640, 360);
  await page.getByRole('button', { name: 'Playback settings' }).click();
  await expect(page.getByTestId('playback-panel').getByTestId('sleep-armed')).toHaveText(
    /30 min/,
  );
});
