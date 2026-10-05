/**
 * Multi-view (README §7.4).
 *
 * The journeys here are the two halves of the feature. One is a mosaic that opens,
 * moves its audio and promotes a tile. The other is the refusal — and it is the one the
 * feature is judged on, because the fixture provider allows **two** connections, so a
 * 2×2 with four channels cannot be opened at all. That is the same arithmetic the real
 * subscription hit with one connection, and it is worth having a journey that asserts
 * the viewer is told the number rather than shown four broken tiles.
 *
 * What these cannot show: the picture. Tile video is a child window composited behind
 * the WebView, so a browser has the chrome, the layout and every decision about them,
 * and no frames at all — the host scenarios are where the surfaces are checked
 * (`AUDIT/test-report.md` §11).
 */
import { expect, test, type Page } from '@playwright/test';

const SHOTS = 'screenshots';

/**
 * The fixture line: two connections, one of them already held by a recording in
 * progress — so exactly **one** tile fits until that recording is stopped. That is not
 * a quirk of the fixture to be worked around, it is the arithmetic this feature is
 * about, so one journey asserts it and the rest stop the recording first.
 */
async function stopTheRecording(page: Page) {
  await page.goto('/#/recordings');
  await expect(page.getByRole('heading', { name: 'Recordings' })).toBeVisible();
  // The one in flight is on the Scheduled tab; Recorded is where the page lands.
  await page.getByRole('tab', { name: /Scheduled/ }).click();
  await page.getByRole('button', { name: 'Stop' }).click();
  await expect(page.getByRole('button', { name: 'Stop' })).toHaveCount(0);
}

async function openMultiview(page: Page) {
  await page.goto('/#/multiview');
  await expect(page.getByRole('heading', { name: 'Multi-view' })).toBeVisible();
}

/**
 * Choose a channel in one tile.
 *
 * Through the filter box rather than by scrolling: `Select` grows one past a dozen
 * options and a real channel list has thousands, so this is also the only way a person
 * would do it.
 */
async function pickTile(page: Page, tile: number, channel: string) {
  await page.getByTestId(`mosaic-pick-${tile}`).getByRole('button').first().click();
  const list = page.getByRole('listbox');
  await list.getByRole('textbox').fill(channel);
  await list.getByRole('option', { name: channel }).first().click();
}

test('a mosaic opens, and one tile has the sound', async ({ page }) => {
  await stopTheRecording(page);
  await openMultiview(page);

  // Two channels on a two-connection line. The other two tiles stay empty, which costs
  // no connections — the whole reason an empty tile is a thing a viewer can have.
  await pickTile(page, 0, 'Meridian News');
  await pickTile(page, 1, 'Apex Sports 1');

  await page.getByTestId('mosaic-open').click();

  const overlay = page.getByTestId('mosaic-overlay');
  await expect(overlay).toBeVisible();
  await page.screenshot({ path: `${SHOTS}/multiview-open.png` });

  // Four tiles for a 2x2, two of them with channels in.
  await expect(page.getByTestId('mosaic-tile-0')).toContainText('Meridian News');
  await expect(page.getByTestId('mosaic-tile-1')).toContainText('Apex Sports 1');
  await expect(page.getByTestId('mosaic-tile-2')).toContainText('Empty');
  await expect(page.getByTestId('mosaic-tile-3')).toContainText('Empty');

  // Exactly one tile has audio, and it starts on the first with a picture.
  await expect(page.getByTestId('mosaic-tile-0')).toHaveAttribute('data-focused', 'true');
  await expect(page.getByTestId('mosaic-tile-1')).toHaveAttribute('data-focused', 'false');

  // `1`-`9` move it (README §7.4). Digits are channel entry everywhere else, which is
  // why this is worth asserting rather than assuming.
  await page.keyboard.press('2');
  await expect(page.getByTestId('mosaic-tile-1')).toHaveAttribute('data-focused', 'true');
  await expect(page.getByTestId('mosaic-tile-0')).toHaveAttribute('data-focused', 'false');

  // And so does a click.
  await page.getByTestId('mosaic-tile-0').click();
  await expect(page.getByTestId('mosaic-tile-0')).toHaveAttribute('data-focused', 'true');
});

test('a recording in flight is counted against the layout, and said so', async ({ page }) => {
  await openMultiview(page);

  // Two connections, one held by a recording: the 2x2 needs four streams and five are
  // in play. The picker says that, and names the recording as part of it, before any
  // channel is chosen.
  const budget = page.getByTestId('mosaic-budget');
  await expect(budget).toContainText('Needs 5');
  await expect(budget).toContainText('allows 2');
  await expect(budget).toContainText('1 recording using the line');
  await page.screenshot({ path: `${SHOTS}/multiview-over-limit.png` });

  // With one connection free, one tile fits and two do not.
  await pickTile(page, 0, 'Meridian News');
  await expect(page.getByTestId('mosaic-open')).toBeEnabled();

  await pickTile(page, 1, 'Apex Sports 1');
  await expect(page.getByTestId('mosaic-open')).toBeDisabled();
  await expect(page.getByText(/3 streams is more than your provider allows/)).toBeVisible();

  // Stopping the recording gives the connection back, and the same selection opens.
  await stopTheRecording(page);
  await openMultiview(page);
  await pickTile(page, 0, 'Meridian News');
  await pickTile(page, 1, 'Apex Sports 1');
  await expect(page.getByTestId('mosaic-open')).toBeEnabled();
});

test('no arrangement fits a line that is too small for any of them', async ({ page }) => {
  await openMultiview(page);
  // Nine tiles plus a recording against two connections. There is no smaller layout
  // that fits either, and saying so is more use than offering a 2x2 that also fails.
  await page.getByTestId('mosaic-layout-grid3x3').click();
  const budget = page.getByTestId('mosaic-budget');
  await expect(budget).toContainText('Needs 10');
  await expect(budget).toContainText('no arrangement fits on this line');
});

test('a tile can be promoted to the main player', async ({ page }) => {
  await stopTheRecording(page);
  await openMultiview(page);
  await pickTile(page, 0, 'Meridian News');
  await pickTile(page, 1, 'Apex Sports 1');
  await page.getByTestId('mosaic-open').click();
  await expect(page.getByTestId('mosaic-overlay')).toBeVisible();

  // Double-click, because a single click is audio focus and both gestures are on the
  // same target.
  await page.getByTestId('mosaic-tile-1').dblclick();

  // The mosaic is gone and the player has that channel.
  await expect(page.getByTestId('mosaic-overlay')).toBeHidden();
  await expect(page.getByRole('button', { name: 'Pause' })).toBeVisible();
  await expect(page.getByText('Apex Sports 1').first()).toBeVisible();
});

test('a layout can be saved, reopened and deleted', async ({ page }) => {
  await stopTheRecording(page);
  await openMultiview(page);
  await pickTile(page, 0, 'Meridian News');
  await page.getByTestId('mosaic-open').click();
  await expect(page.getByTestId('mosaic-overlay')).toBeVisible();

  await page.getByTestId('mosaic-save').click();
  await page.getByRole('dialog', { name: 'Save this layout' }).getByRole('textbox').fill('Sunday');
  await page.getByTestId('mosaic-save-confirm').click();
  await expect(page.getByTestId('mosaic-save-dialog')).toBeHidden();

  // Back on the picker, the saved layout is listed and opens again.
  await page.getByTestId('mosaic-close').click();
  await openMultiview(page);
  const row = page.getByTestId(/^mosaic-saved-/).first();
  await expect(row).toContainText('Sunday');

  await row.getByRole('button', { name: 'Open' }).click();
  await expect(page.getByTestId('mosaic-overlay')).toBeVisible();
  await expect(page.getByTestId('mosaic-tile-0')).toContainText('Meridian News');

  await page.getByTestId('mosaic-close').click();
  await openMultiview(page);
  await page.getByRole('button', { name: 'Delete Sunday' }).click();
  await expect(page.getByTestId(/^mosaic-saved-/)).toHaveCount(0);
});

test('closing the mosaic gives the shell back', async ({ page }) => {
  await stopTheRecording(page);
  await openMultiview(page);
  await pickTile(page, 0, 'Meridian News');
  await page.getByTestId('mosaic-open').click();
  await expect(page.getByTestId('mosaic-overlay')).toBeVisible();

  // While a mosaic is up the shell is transparent, because there is video behind the
  // whole client area — the same rule the player follows, and the same hole through to
  // the desktop if it is got wrong.
  const shell = page.getByTestId('app-shell');
  await expect(shell).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)');

  await page.keyboard.press('Escape');
  await expect(page.getByTestId('mosaic-overlay')).toBeHidden();
  await expect(shell).not.toHaveCSS('background-color', 'rgba(0, 0, 0, 0)');
});
