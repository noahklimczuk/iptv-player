/**
 * The seek preview: "add a seek preview to the player", settled as frames and time.
 *
 * The browser build's host is the mock, which draws a card for the parts already
 * played -- the same rule the real host keeps, since it only has frames of those -- so
 * what is asserted here is the bubble: that it follows the pointer, shows a picture
 * only where one exists, always shows the time, and on live TV the time behind live and
 * the programme.
 */
import { expect, test, type Page } from '@playwright/test';

async function playFirstEpisode(page: Page) {
  await page.goto('/#/series');
  await expect(page.getByRole('heading', { name: 'Series' })).toBeVisible();
  await page.locator('[role="button"]').first().click();
  const dialog = page.getByRole('dialog');
  await expect(dialog).toBeVisible();
  await dialog.getByRole('tab', { name: 'episodes' }).click();
  await dialog.locator('button', { hasText: /^\d/ }).first().click();
  await expect(page.getByRole('button', { name: 'Pause' })).toBeVisible();
}

/** The dock slides in over the page it was opened from; wait until it is really there. */
async function settled(page: Page, name: string) {
  const s = page.getByRole('slider', { name });
  await expect.poll(async () => {
    const b = await s.boundingBox();
    if (!b) return false;
    return s.evaluate((el, [x, y]) => document.elementFromPoint(x, y) === el,
      [b.x + b.width / 2, b.y + b.height / 2] as [number, number]);
  }).toBe(true);
  return (await s.boundingBox())!;
}

/** Hover the bar `fraction` of the way along. */
async function hoverAt(page: Page, slider: string, fraction: number) {
  // Wake the controls the way a viewer would. They hide 3.2 s after the last input, and
  // the first hover here comes after a three-second wait with none.
  await page.mouse.move(640, 300);
  await page.mouse.move(645, 305);
  const box = await settled(page, slider);
  await page.mouse.move(box.x + box.width * fraction, box.y + box.height / 2);
}

const bubble = (page: Page) => page.getByTestId('seek-preview');

test('hovering a film shows a picture of what has played, and only the time ahead', async ({ page }) => {
  await playFirstEpisode(page);
  // Let a little of it play, so there is something behind the playhead.
  await page.waitForTimeout(3_000);
  const bar = page.getByRole('slider', { name: 'Seek' });
  const max = Number(await bar.getAttribute('max'));
  const played = Number(await bar.inputValue());
  expect(played).toBeGreaterThan(0);

  // Just behind the playhead: a picture and its time.
  await hoverAt(page, 'Seek', (played * 0.5) / max);
  await expect(bubble(page)).toBeVisible();
  await expect(page.getByTestId('seek-preview-frame')).toBeVisible();
  await expect(page.getByTestId('seek-preview-time')).toHaveText(/^\d+:\d{2}/);

  // Far ahead: not played, so no picture -- but still the time.
  await hoverAt(page, 'Seek', 0.9);
  await expect(page.getByTestId('seek-preview-frame')).toBeHidden();
  await expect(page.getByTestId('seek-preview-time')).toHaveText(/^\d+:\d{2}/);

  // The bubble sits over the pointer, not at a fixed place.
  const near = (await bubble(page).boundingBox())!;
  await hoverAt(page, 'Seek', 0.5);
  const mid = (await bubble(page).boundingBox())!;
  expect(Math.abs(near.x - mid.x)).toBeGreaterThan(20);

  // And goes when the pointer does.
  await page.mouse.move(10, 10);
  await expect(bubble(page)).toBeHidden();
});

test('a marked stretch is named in the bubble', async ({ page }) => {
  await playFirstEpisode(page);
  const bar = page.getByRole('slider', { name: 'Seek' });
  const max = Number(await bar.getAttribute('max'));
  // The mock calls 30 s to 90 s the intro.
  await hoverAt(page, 'Seek', 60 / max);
  await expect(bubble(page)).toContainText('Intro');
});

test('on live TV the bubble says how far behind, when, and what was on', async ({ page }) => {
  await page.goto('/#/live');
  await expect(page.getByRole('heading', { name: 'Live TV' })).toBeVisible();
  await page.getByRole('button', { name: 'Watch Meridian News' }).first().click();
  await expect(page.getByRole('button', { name: 'Pause' })).toBeVisible();
  await page.waitForTimeout(6_000);
  await page.mouse.move(800, 500);

  await hoverAt(page, 'Timeshift', 0.1);
  await expect(bubble(page)).toBeVisible();
  await expect(page.getByTestId('seek-preview-time')).toHaveText(/^−\d+:\d{2}/);
  await expect(bubble(page)).toContainText('Evening News');
  await expect(bubble(page)).toContainText(/\d{1,2}:\d{2}/);
  await expect(page.getByTestId('seek-preview-frame')).toBeVisible();
});
