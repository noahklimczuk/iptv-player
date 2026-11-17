/**
 * "The player is still kinda broken, the controls don't really work."
 *
 * What was found, pressing every control against the real host and watching its state:
 *
 * - The dock hid 3.2 s after the mouse last *moved*, wherever it was. A pointer resting
 *   on the volume bar lost it from under it, and a seek handle held still for three
 *   seconds was unmounted mid-drag with the seek never sent.
 * - Touching a slider left it focused, and the shortcuts treated any `<input>` as a
 *   text field: Space and M did nothing after that, and the arrows moved the slider.
 * - One drag of the seek bar sent two seeks.
 * - Full screen from the guide's preview re-tuned a channel that was already playing.
 */
import { expect, test, type Page } from '@playwright/test';

interface MockCall { name: string; args: Record<string, unknown>; bytes: number }

const calls = (page: Page, name: string) => page.evaluate(
  (n) => (window as unknown as { __auroraCalls: () => MockCall[] }).__auroraCalls()
    .filter((c) => c.name === n).map((c) => c.args),
  name,
);
const clearCalls = (page: Page) => page.evaluate(
  () => (window as unknown as { __auroraClearCalls: () => void }).__auroraClearCalls(),
);
const state = (page: Page) => page.evaluate(async () => {
  const w = window as unknown as { __auroraInvoke: (c: string) => Promise<{ status: string; muted: boolean; volume: number; positionSecs: number }> };
  return w.__auroraInvoke('player.state');
});

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

test('the controls stay up a while under a resting pointer, then still get out of the way', async ({ page }) => {
  await playFirstEpisode(page);
  const volume = page.getByRole('slider', { name: 'Volume' });
  const box = await settled(page, 'Volume');
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await page.waitForTimeout(3_500);
  await expect(volume, 'still there under a pointer that has only just stopped').toBeVisible();
  // "The auto hide takes a long time to go": a pointer left on the dock after a click
  // used to hold it over the film for as long as it stayed there.
  await page.waitForTimeout(2_500);
  await expect(volume, 'gone once the pointer has rested a while, even on the dock').toBeHidden();
});

test('over the picture the controls hide within a few seconds', async ({ page }) => {
  await playFirstEpisode(page);
  const volume = page.getByRole('slider', { name: 'Volume' });
  await settled(page, 'Volume');
  await page.mouse.move(700, 300);
  await page.mouse.move(705, 305);
  await page.waitForTimeout(4_000);
  await expect(volume).toBeHidden();
  // And the first movement brings them straight back.
  await page.mouse.move(720, 320);
  await expect(volume).toBeVisible();
});

test('an open menu stays while the pointer is on it', async ({ page }) => {
  await playFirstEpisode(page);
  await settled(page, 'Volume');
  await page.getByRole('button', { name: 'Playback settings' }).click();
  const panel = page.getByTestId('playback-panel');
  const box = (await panel.boundingBox())!;
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await page.waitForTimeout(6_000);
  await expect(panel).toBeVisible();
});

test('a seek handle held still is not lost, and one drag sends one seek', async ({ page }) => {
  await playFirstEpisode(page);
  const bar = page.getByRole('slider', { name: 'Seek' });
  const box = await settled(page, 'Seek');
  await clearCalls(page);
  await page.mouse.move(box.x + box.width * 0.2, box.y + box.height / 2);
  await page.mouse.down();
  await page.mouse.move(box.x + box.width * 0.5, box.y + box.height / 2, { steps: 8 });
  // Held, without moving, past the moment the dock used to hide.
  await page.waitForTimeout(4_000);
  await page.mouse.move(box.x + box.width * 0.6, box.y + box.height / 2, { steps: 4 });
  await page.mouse.up();
  await page.waitForTimeout(600);

  const seeks = await calls(page, 'player.seek');
  expect(seeks, 'exactly one seek for one drag').toHaveLength(1);
  const max = Number(await bar.getAttribute('max'));
  expect(Number(seeks[0]!.positionSecs)).toBeGreaterThan(max * 0.5);
});

test('after touching a slider, Space pauses, M mutes, and the arrows seek', async ({ page }) => {
  await playFirstEpisode(page);
  const volume = page.getByRole('slider', { name: 'Volume' });
  const vb = await settled(page, 'Volume');
  await page.mouse.click(vb.x + vb.width * 0.3, vb.y + vb.height / 2);
  await expect(volume).toBeFocused();
  // The click's volume is sent on the next frame; read it once the host has it.
  await expect.poll(async () => (await state(page)).volume).toBe(Number(await volume.inputValue()));
  const level = (await state(page)).volume;

  await page.keyboard.press('Space');
  await expect.poll(async () => (await state(page)).status).toBe('paused');
  await page.keyboard.press('Space');
  await expect.poll(async () => (await state(page)).status).toBe('playing');

  await page.keyboard.press('m');
  await expect.poll(async () => (await state(page)).muted).toBe(true);
  await page.keyboard.press('m');
  await expect.poll(async () => (await state(page)).muted).toBe(false);

  await clearCalls(page);
  await page.keyboard.press('ArrowRight');
  await expect.poll(async () => (await calls(page, 'player.seek')).length).toBe(1);
  expect((await state(page)).volume, 'the arrow seeked; it did not nudge the volume bar').toBe(level);
});

test('Space on a focused button presses it once, not twice', async ({ page }) => {
  await playFirstEpisode(page);
  const pause = page.getByRole('button', { name: 'Pause' });
  await pause.focus();
  await page.keyboard.press('Space');
  await expect.poll(async () => (await state(page)).status).toBe('paused');
});

test('Full screen from the guide preview shows the channel without tuning it again', async ({ page }) => {
  await page.goto('/#/guide');
  const first = page.getByTestId('guide-channel').first();
  await first.click();
  await expect(first).toHaveAttribute('data-live', 'true');
  await clearCalls(page);
  await page.getByRole('button', { name: 'Full screen' }).click();
  await expect(page.getByRole('button', { name: 'Pause' })).toBeVisible();
  expect(await calls(page, 'player.play'), 'no second tune of a channel already on').toHaveLength(0);
});
