/**
 * Two ways Live TV showed a list that no control on screen could explain.
 *
 * The A–Z bar is greyed out unless the list is alphabetical, and the filter it set was
 * applied regardless of the order. So: press A–Z, pick a letter, press Number — the list
 * is still only that letter's channels, the count beside the heading still reflects them,
 * and the bar that would clear it is now `pointerEvents: 'none'`. The only other way out
 * is the "Clear search" button inside the no-matches empty state, which needs
 * `visible.length === 0` — unavailable in exactly the case where something *is* showing
 * and most of the group is missing (R-112).
 *
 * And the grid's tile width was measured by a ref callback, which fires on attach and
 * detach and never on resize. CSS `auto-fill` then lays out one number of tiles per row
 * while the virtualiser assumes another, so its total height is wrong and rows land at
 * the wrong offsets (R-113).
 */
import { expect, test, type Page } from '@playwright/test';

async function live(page: Page) {
  await page.goto('/#/live');
  await expect(page.getByRole('heading', { name: 'Live TV' })).toBeVisible();
  await expect(page.getByTestId('channel-row').first()).toBeVisible();
}

/** How many channels the screen says it is showing. */
async function shown(page: Page): Promise<number> {
  const text = await page.getByTestId('channel-count').innerText();
  return Number(text.replace(/[^0-9]/g, ''));
}

test('leaving A–Z does not leave the letter filter behind', async ({ page }) => {
  await live(page);
  const all = await shown(page);
  expect(all).toBeGreaterThan(1);

  await page.getByRole('button', { name: 'A–Z', exact: true }).click();
  const bar = page.getByRole('group', { name: 'Jump to letter' });
  await expect(bar).toBeVisible();

  // A letter that narrows the list to something smaller than the whole group.
  let picked = '';
  for (const letter of 'ABCDEFGHIJKLMNOPQRSTUVWXYZ') {
    await bar.getByRole('button', { name: letter, exact: true }).click();
    const n = await shown(page);
    if (n > 0 && n < all) {
      picked = letter;
      break;
    }
  }
  expect(picked, 'no letter narrowed the list').not.toBe('');
  const narrowed = await shown(page);

  // Back to Number, where the bar is unavailable.
  await page.getByRole('button', { name: 'Number', exact: true }).click();
  await expect(bar).toHaveAttribute('aria-disabled', 'true');

  // The list is the whole group again, because nothing on screen could have cleared it.
  await expect
    .poll(() => shown(page), { message: `still showing the ${picked} channels` })
    .toBe(all);
  expect(narrowed).toBeLessThan(all);
});

test('the greyed-out letter bar cannot be used by keyboard either', async ({ page }) => {
  await live(page);
  const bar = page.getByRole('group', { name: 'Jump to letter' });
  await expect(bar).toHaveAttribute('aria-disabled', 'true');

  // `pointer-events: none` leaves an element in the tab order and does not stop Enter
  // activating it, so every one of these was reachable and fired while looking inert.
  const buttons = bar.getByRole('button');
  const n = await buttons.count();
  expect(n).toBeGreaterThan(20);
  for (const i of [0, 1, Math.floor(n / 2), n - 1]) {
    // `toBeDisabled` would be satisfied by the group's own `aria-disabled`, which is
    // what says it out loud; this asserts the thing that actually stops the key.
    await expect(buttons.nth(i)).toHaveJSProperty('disabled', true);
  }
});

test('the grid re-measures when the window changes size', async ({ page }) => {
  await live(page);
  await page.getByRole('button', { name: 'Grid', exact: true }).click();
  await expect(page.getByTestId('channel-tile').first()).toBeVisible();

  /** How many tiles share the topmost row's y position. */
  const perRow = async () => {
    const boxes = await page.getByTestId('channel-tile').evaluateAll((els) =>
      els.map((el) => el.getBoundingClientRect().top));
    const top = Math.min(...boxes);
    return boxes.filter((t) => Math.abs(t - top) < 2).length;
  };

  const wide = await perRow();
  expect(wide).toBeGreaterThan(1);

  // Narrow enough that `auto-fill` fits fewer tiles across.
  await page.setViewportSize({ width: 700, height: 900 });
  await expect.poll(perRow).toBeLessThan(wide);

  // And the virtualiser agrees with the layout: no two tiles overlap, which is what a
  // stale `perRow` produces — CSS puts four across while `rowCount` assumes six.
  const overlapping = await page.getByTestId('channel-tile').evaluateAll((els) => {
    const r = els.map((el) => el.getBoundingClientRect());
    let bad = 0;
    for (let i = 0; i < r.length; i += 1) {
      for (let j = i + 1; j < r.length; j += 1) {
        const a = r[i]!;
        const b = r[j]!;
        const overlap = a.left < b.right - 1 && b.left < a.right - 1
          && a.top < b.bottom - 1 && b.top < a.bottom - 1;
        if (overlap) bad += 1;
      }
    }
    return bad;
  });
  expect(overlapping, 'tiles are on top of each other').toBe(0);
});
