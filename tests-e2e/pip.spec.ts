/**
 * Picture-in-picture (README §6.2, §14.1 `P`).
 *
 * What a browser can show: that `P` moves the picture into a corner, that the shell stays
 * usable while it is there — which is the entire point — that the corner cycles, and that
 * expanding puts the full player back.
 *
 * What it cannot show: the picture. The video is the main player's surface, moved by the
 * host behind a transparent WebView, so a browser has the frame and the hole and no
 * frames at all.
 */
import { expect, test, type Page } from '@playwright/test';

const SHOTS = 'screenshots';

const tile = (page: Page) => page.getByTestId('pip-tile');

async function playSomething(page: Page) {
  await page.goto('/#/live');
  await expect(page.getByRole('heading', { name: 'Live TV' })).toBeVisible();
  await page.getByRole('button', { name: 'Watch Meridian News' }).first().click();
  await expect(page.getByRole('button', { name: 'Pause' })).toBeVisible();
}

test('P puts the picture in a corner and leaves the app usable', async ({ page }) => {
  await playSomething(page);
  await expect(tile(page)).toBeHidden();

  await page.keyboard.press('p');
  await expect(tile(page)).toBeVisible();
  await page.screenshot({ path: `${SHOTS}/pip-tile.png` });

  // The full-screen player stands down: its transport is what would otherwise be
  // covering the page.
  await expect(page.getByRole('button', { name: 'Pause' })).toBeHidden();

  // And the point of the feature — the app is still there to be used.
  await expect(page.getByRole('navigation', { name: 'Main' })).toBeVisible();
  await page.getByRole('navigation', { name: 'Main' }).getByRole('link', { name: 'Movies' }).click();
  await expect(page.getByRole('heading', { name: 'Movies' })).toBeVisible();
  await expect(tile(page), 'the picture did not survive navigating').toBeVisible();
});

test('the tile sits in a corner, and moves to the next one', async ({ page }) => {
  await playSomething(page);
  await page.keyboard.press('p');
  await expect(tile(page)).toHaveAttribute('data-corner', 'bottomRight');

  const viewport = page.viewportSize()!;
  const box = (await tile(page).boundingBox())!;
  // Bottom right: past the middle on both axes, and inside the window.
  expect(box.x).toBeGreaterThan(viewport.width / 2);
  expect(box.x + box.width).toBeLessThanOrEqual(viewport.width);
  expect(box.y + box.height).toBeLessThanOrEqual(viewport.height);

  // 16:9, so it does not change shape when the viewer zaps to a 4:3 channel.
  expect(box.width / box.height).toBeCloseTo(16 / 9, 1);

  await page.getByTestId('pip-corner').click();
  await expect(tile(page)).toHaveAttribute('data-corner', 'bottomLeft');
  const moved = (await tile(page).boundingBox())!;
  expect(moved.x).toBeLessThan(viewport.width / 2);
});

test('expanding gives the full player back', async ({ page }) => {
  await playSomething(page);
  await page.keyboard.press('p');
  await expect(tile(page)).toBeVisible();

  await page.getByTestId('pip-expand').click();
  await expect(tile(page)).toBeHidden();
  await expect(page.getByRole('button', { name: 'Pause' })).toBeVisible();
});

test('closing the tile stops playback rather than leaving it running unseen', async ({
  page,
}) => {
  await playSomething(page);
  await page.keyboard.press('p');
  await expect(tile(page)).toBeVisible();

  await page.getByTestId('pip-close').click();
  await expect(tile(page)).toBeHidden();
  // A stream still running behind a closed tile is a connection nobody can see they are
  // using, which on a one-connection line is the difference between the next tune
  // working and not.
  await expect(page.getByRole('button', { name: 'Pause' })).toBeHidden();
});

test('multi-view and the small picture do not fight over the surface', async ({ page }) => {
  await page.goto('/#/multiview');
  await expect(page.getByRole('heading', { name: 'Multi-view' })).toBeVisible();

  // Open a mosaic with the one connection the fixture's recording leaves free.
  await page.getByTestId('mosaic-pick-0').getByRole('button').first().click();
  const list = page.getByRole('listbox');
  await list.getByRole('textbox').fill('Meridian News');
  await list.getByRole('option', { name: 'Meridian News' }).first().click();
  await page.getByTestId('mosaic-open').click();
  await expect(page.getByTestId('mosaic-overlay')).toBeVisible();

  // `P` is refused while a mosaic is up: both want the same surfaces in the same window.
  await page.keyboard.press('p');
  await expect(tile(page)).toBeHidden();
  await expect(page.getByText(/Close multi-view first/i)).toBeVisible();
});

/**
 * The hole, which is the part that was never there.
 *
 * Every test above passed while the small picture showed nothing at all, because they
 * check where the *tile* is and the tile is a frame — the picture is mpv, behind the
 * WebView, and whether it is visible is a question about what the page paints over it.
 *
 * The shell is opaque while PiP is on, deliberately: the point is to read a page with the
 * stream still running. An opaque layer cannot be given a see-through rectangle by
 * putting a transparent child in it, which is what the old four-bands approach amounted
 * to — and the bands were above the content as well, so they covered the page they were
 * meant to leave readable.
 *
 * Clipping the shell is the version that works, and hit-testing is how a browser can be
 * asked whether it worked: a region removed by `clip-path` does not hit-test, so the
 * shell must be absent from the stack of elements under the middle of the tile and
 * present everywhere else.
 */
test('the shell is cut away behind the tile, and nowhere else', async ({ page }) => {
  await page.goto('/?video#/live');
  await expect(page.getByRole('heading', { name: 'Live TV' })).toBeVisible();
  await page.getByRole('button', { name: 'Watch Meridian News' }).first().click();
  await expect(page.getByRole('button', { name: 'Pause' })).toBeVisible();
  await page.keyboard.press('p');
  await expect(page.getByTestId('pip-tile')).toBeVisible();

  const box = (await page.getByTestId('pip-tile').boundingBox())!;

  const stacks = await page.evaluate((b) => {
    const names = (x: number, y: number) =>
      document.elementsFromPoint(x, y).map((el) => el.getAttribute('data-testid') ?? el.tagName);
    return {
      inside: names(b.x + b.width / 2, b.y + b.height / 2),
      // Well away from the tile, where the page must still be solid.
      outside: names(40, 40),
      shellClip: getComputedStyle(
        document.querySelector('[data-testid="app-shell"]') as HTMLElement,
      ).clipPath,
      // The tile has to be outside the clipped subtree or it would be clipped with it.
      tileInShell: !!document
        .querySelector('[data-testid="app-shell"]')
        ?.contains(document.querySelector('[data-testid="pip-tile"]')),
    };
  }, box);

  // The frame is there…
  expect(stacks.inside).toContain('pip-tile');
  // …and the shell is not, which is the whole point: nothing of the page is painted in
  // that rectangle, so what is behind the WebView shows through.
  expect(
    stacks.inside,
    'the app shell still covers the tile, so the picture cannot show through',
  ).not.toContain('app-shell');

  // Everywhere else the page is untouched — a hole that swallowed the window would be
  // just as broken as no hole at all.
  expect(stacks.outside).toContain('app-shell');

  expect(stacks.shellClip).toContain('evenodd');
  expect(stacks.tileInShell, 'the tile is inside the clipped subtree').toBe(false);
});

test('the hole is gone once the picture is not in a corner', async ({ page }) => {
  await page.goto('/?video#/live');
  await page.getByRole('button', { name: 'Watch Meridian News' }).first().click();
  await expect(page.getByRole('button', { name: 'Pause' })).toBeVisible();

  const shellClip = () =>
    page.evaluate(
      () =>
        getComputedStyle(document.querySelector('[data-testid="app-shell"]') as HTMLElement)
          .clipPath,
    );

  await page.keyboard.press('p');
  await expect(page.getByTestId('pip-tile')).toBeVisible();
  expect(await shellClip()).toContain('polygon');

  // Back to full screen: `videoBehind` makes the whole shell transparent for that, and a
  // clip on top of it would be a hole in a window that is already a hole.
  await page.getByTestId('pip-expand').click();
  await expect(page.getByTestId('pip-tile')).toBeHidden();
  expect(await shellClip()).toBe('none');
});
