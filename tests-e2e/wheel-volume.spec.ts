/**
 * Volume on the mouse wheel, over the picture.
 *
 * The only pointer route to the volume was the slider inside the OSD chrome, which hides
 * itself after 3.2 seconds — so turning the volume down with a mouse meant waking the
 * overlay first and then finding a 100-pixel range input. A wheel over the video is what
 * every other player does.
 */
import { expect, test, type Page } from '@playwright/test';

/** Open the player on a live channel, with the OSD up. */
async function watchLive(page: Page) {
  await page.goto('/#/live');
  await expect(page.getByTestId('channel-row').first()).toBeVisible();
  await page.getByTestId('channel-row').first().click();
  await expect(page.getByRole('button', { name: 'Pause' })).toBeVisible();
  await page.mouse.move(640, 360);
}

test('the wheel turns the volume down and up over the picture', async ({ page }) => {
  await watchLive(page);

  const slider = page.getByRole('slider', { name: 'Volume' });
  await expect(slider).toBeVisible();
  const before = Number(await slider.inputValue());
  expect(before).toBeGreaterThan(10);

  // Down, by one notch's worth of travel.
  await page.mouse.move(640, 360);
  await page.mouse.wheel(0, 100);
  await expect
    .poll(async () => Number(await slider.inputValue()))
    .toBeLessThan(before);

  const quieter = Number(await slider.inputValue());

  // And back up, which must return it rather than merely move it again.
  await page.mouse.wheel(0, -100);
  await expect
    .poll(async () => Number(await slider.inputValue()))
    .toBeGreaterThan(quieter);
});

test('a flick of the wheel does not slam the volume to an end', async ({ page }) => {
  await watchLive(page);
  const slider = page.getByRole('slider', { name: 'Volume' });
  const before = Number(await slider.inputValue());

  // A trackpad's stream of small deltas. Acting on each event is what this guards
  // against: ten of these must be worth about two notches, not ten.
  await page.mouse.move(640, 360);
  for (let i = 0; i < 10; i += 1) await page.mouse.wheel(0, 10);

  await expect
    .poll(async () => Number(await slider.inputValue()))
    .toBeLessThan(before);
  const after = Number(await slider.inputValue());
  expect(after).toBeGreaterThan(before - 20);
  expect(after).toBeGreaterThan(0);
});

test('the volume stops at zero rather than erroring', async ({ page }) => {
  await watchLive(page);
  const slider = page.getByRole('slider', { name: 'Volume' });

  // Far past the bottom. The host's `volume` is a `u32`, so a negative one is refused by
  // serde and the catch turns that into a toast — an error for an action that had
  // already finished (R-85).
  await page.mouse.move(640, 360);
  for (let i = 0; i < 40; i += 1) await page.mouse.wheel(0, 100);

  await expect.poll(async () => Number(await slider.inputValue())).toBe(0);
  await expect(page.getByRole('alert')).toHaveCount(0);
});

test('the wheel does nothing when it is aimed at the controls', async ({ page }) => {
  await page.goto('/#/movies');
  await expect(page.getByTestId('catalog-card').first()).toBeVisible();
  await page.getByTestId('catalog-card').first().getByRole('button').first().click();
  await page.getByRole('dialog').getByRole('button', { name: /^(Play|Resume)/ }).click();
  await expect(page.getByRole('button', { name: 'Pause' })).toBeVisible();
  await page.mouse.move(640, 360);

  await page.getByRole('button', { name: 'Playback settings' }).click();
  const panel = page.getByTestId('playback-panel');
  await expect(panel).toBeVisible();

  const slider = page.getByRole('slider', { name: 'Volume' });
  const before = Number(await slider.inputValue());

  // Over the panel, not over the picture. The overlay covers the whole window, so
  // without a rule the wheel reaches the controls as readily as the video — and a wheel
  // that changes the volume wherever you point it is not a shortcut. Deliberately not
  // conditional on whether this panel overflows: the rule is about what was aimed at.
  const box = await panel.boundingBox();
  if (!box) throw new Error('the playback panel has no box to aim at');
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await page.mouse.wheel(0, 100);

  // Positive assertion rather than only "it did not change": the volume is still what it
  // was *and* the panel is still the thing on screen, so this cannot pass against a
  // player that closed itself.
  await expect(panel).toBeVisible();
  expect(Number(await slider.inputValue())).toBe(before);
});
