/**
 * The boot screen when the host has gone quiet.
 *
 * This is the screen 1.0.1 left people looking at. The window came up, React mounted,
 * and then every command the first screen makes stopped returning — so `BootScreen` sat
 * on "Opening your library…" under a sweeping progress bar, indefinitely, saying
 * everything was fine. There was no error on screen, nothing in a log a viewer could
 * reach, and no way out but the task manager.
 *
 * The bug that caused it is fixed elsewhere. This is about the screen: a launch that
 * never completes has to end up somewhere a person can act, because the next cause of
 * one will not be the same cause.
 *
 * Needs two seams. `?hang=` makes a command never answer, which is different from making
 * it refuse — a refusal is an answer, and the UI already had somewhere to put one, which
 * is why every existing failure test passed while this state was unreachable. `?bootStuck`
 * shortens the screen's patience so the assertion does not cost twenty seconds.
 */
import { expect, test } from '@playwright/test';

const SHOTS = 'screenshots';

/** `providers.list` is one of the two commands that gate the first screen. */
const HUNG = '/?hang=providers.list&bootStuck=1200#/';

test('a launch that never finishes says so, and offers a way out', async ({ page }) => {
  await page.goto(HUNG);

  // First it is simply loading, which is the correct thing to show at this point.
  await expect(page.getByTestId('boot-screen')).toBeVisible();
  await expect(page.getByTestId('boot-stuck')).toBeHidden();

  // Then it stops claiming to be loading.
  await expect(page.getByTestId('boot-stuck')).toBeVisible();
  await expect(page.getByText(/isn’t getting an answer/)).toBeVisible();
  await expect(page.getByRole('button', { name: 'Reload' })).toBeVisible();
  await page.screenshot({ path: `${SHOTS}/boot-stuck.png` });

  // The reassurance is gone: it is not both "a large library takes a moment" and
  // "no answer".
  await expect(page.getByText('A large library takes a moment to open.')).toBeHidden();
});

test('the reassurance still appears on a launch that is merely slow', async ({ page }) => {
  // Hung, but with the real patience, so the first tier is what shows.
  await page.goto('/?hang=providers.list#/');
  await expect(page.getByText('A large library takes a moment to open.')).toBeVisible();
  await expect(page.getByTestId('boot-stuck')).toBeHidden();
});

test('an ordinary launch shows neither', async ({ page }) => {
  await page.goto('/#/');
  // Straight past the boot screen to a real one.
  await expect(page.getByRole('region', { name: 'Featured' })).toBeVisible();
  await expect(page.getByTestId('boot-screen')).toBeHidden();
  await expect(page.getByTestId('boot-stuck')).toBeHidden();
});
