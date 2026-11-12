/**
 * Multi-view on a line that cannot carry it (README §7.4).
 *
 * The fixture provider allows **two** connections and one is held by a recording, so a
 * 2×2 with four channels is refused outright — the same arithmetic the real subscription
 * hits with one. `multiview.spec.ts` asserts that refusal. This asserts the way out of
 * it: the viewer cannot have four streams, but they can have four channels, one at a
 * time, with the other three showing what is on them.
 *
 * What this cannot show, like every mosaic journey: the picture. Tile video is a child
 * window composited behind the WebView, so a browser sees the chrome, the layout and
 * every decision about them, and no frames at all. Which is exactly what is being
 * tested here — a rotating mosaic is mostly chrome, and the chrome is the feature.
 */
import { expect, test, type Page } from '@playwright/test';

const SHOTS = 'screenshots';

async function openMultiview(page: Page) {
  await page.goto('/#/multiview');
  await expect(page.getByRole('heading', { name: 'Multi-view' })).toBeVisible();
}

/**
 * Choose a channel in one tile, through the filter box — the same way `multiview.spec`
 * does it, and the only way a person would with thousands of channels.
 */
async function pickTile(page: Page, tile: number, channel: string) {
  await page.getByTestId(`mosaic-pick-${tile}`).getByRole('button').first().click();
  const list = page.getByRole('listbox');
  await list.getByRole('textbox').fill(channel);
  await list.getByRole('option', { name: channel }).first().click();
}

test('a line that cannot carry the layout is offered the rotation instead', async ({ page }) => {
  await openMultiview(page);

  // Nothing picked yet, and nothing offered: the offer is about a selection.
  await expect(page.getByTestId('mosaic-rotation-offer')).toHaveCount(0);

  await pickTile(page, 0, 'Meridian News');
  await pickTile(page, 1, 'Apex Sports 1');

  // Three streams on a two-connection line with a recording running. Open is refused —
  // and now there is something beside it rather than only the arithmetic.
  await expect(page.getByTestId('mosaic-open')).toBeDisabled();
  const offer = page.getByTestId('mosaic-rotation-offer');
  await expect(offer).toBeVisible();
  await expect(offer).toContainText('one at a time');
  await page.screenshot({ path: `${SHOTS}/multiview-rotation-offer.png` });

  await page.getByTestId('mosaic-open-rotating').click();

  // It opened, which the ordinary Open could not do with this selection.
  const overlay = page.getByTestId('mosaic-overlay');
  await expect(overlay).toBeVisible();
  await expect(page.getByTestId('mosaic-rotation-status')).toContainText(/Next tile in \d+s/);
  await page.screenshot({ path: `${SHOTS}/multiview-rotating.png` });
});

test('the tiles waiting their turn say what is on them', async ({ page }) => {
  await openMultiview(page);
  await pickTile(page, 0, 'Meridian News');
  await pickTile(page, 1, 'Apex Sports 1');
  await page.getByTestId('mosaic-open-rotating').click();
  await expect(page.getByTestId('mosaic-overlay')).toBeVisible();

  // One tile holds the picture and the other is waiting — which the label says, because
  // a tile with no picture and no explanation is the thing this replaced.
  const waiting = page.getByTestId('mosaic-tile-1');
  await expect(waiting).toHaveAttribute('aria-label', /waiting its turn/);
  // And it carries its channel, so the wall is readable rather than eight blank cells.
  await expect(waiting).toContainText('Apex Sports 1');
});

test('clicking a tile stays on it, and the rotation can be started again', async ({ page }) => {
  await openMultiview(page);
  await pickTile(page, 0, 'Meridian News');
  await pickTile(page, 1, 'Apex Sports 1');
  await page.getByTestId('mosaic-open-rotating').click();
  await expect(page.getByTestId('mosaic-overlay')).toBeVisible();

  const status = page.getByTestId('mosaic-rotation-status');
  await expect(status).toContainText(/Next tile in/);

  // Clicking a waiting tile asks for its picture, and asking for it means "stay here".
  // A rotation that carried on would take it away again, which is the opposite of what
  // pressing the tile says.
  await page.getByTestId('mosaic-tile-1').click();
  await expect(status).toContainText('Staying on Apex Sports 1');
  await expect(status).not.toContainText('Next tile in');
  await page.screenshot({ path: `${SHOTS}/multiview-rotation-pinned.png` });

  await page.getByTestId('mosaic-resume-rotation').click();
  await expect(status).toContainText(/Next tile in \d+s/);
});

test('a mosaic that fits is not offered a rotation it does not need', async ({ page }) => {
  // The recording is what makes the line too small; stopping it leaves room for two
  // tiles, and a line that can carry the layout should give every tile its own picture.
  await page.goto('/#/recordings');
  await expect(page.getByRole('heading', { name: 'Recordings' })).toBeVisible();
  await page.getByRole('tab', { name: /Scheduled/ }).click();
  await page.getByRole('button', { name: 'Stop' }).click();
  await expect(page.getByRole('button', { name: 'Stop' })).toHaveCount(0);

  await openMultiview(page);
  await pickTile(page, 0, 'Meridian News');
  await pickTile(page, 1, 'Apex Sports 1');

  await expect(page.getByTestId('mosaic-open')).toBeEnabled();
  await expect(page.getByTestId('mosaic-rotation-offer')).toHaveCount(0);

  await page.getByTestId('mosaic-open').click();
  await expect(page.getByTestId('mosaic-overlay')).toBeVisible();
  // No countdown, because nothing is going to move.
  await expect(page.getByTestId('mosaic-rotation-status')).toHaveCount(0);
});
