/**
 * Arrowing through the palette, which moved a highlight nobody could see.
 *
 * The results list is a 52vh scroller and the highlight was a background colour keyed on
 * a numeric `cursor`. With six groups showing four hits each plus their "Show N more"
 * rows the list is routinely taller than that — on a real subscription a common word
 * matches channels, both guide groups, films and series at once — so arrowing down past
 * the fold moved the highlight to a row that was not on screen. The panel looked like it
 * had stopped responding, and Enter opened something the viewer could not see. README §12
 * asks for the opposite in as many words: "Focus is never lost, never invisible, and
 * always scrolled into view."
 *
 * Nothing was announced either: the rows were plain buttons with no `aria-selected`, the
 * container had no `role="listbox"`, and the input no `aria-activedescendant`, so a
 * screen-reader user pressing Down heard nothing change (R-147).
 */
import { expect, test, type Page } from '@playwright/test';

async function openPalette(page: Page, query: string) {
  await page.goto('/#/');
  await expect(page.getByRole('region', { name: 'Featured' })).toBeVisible();
  await page.keyboard.press('Control+k');
  const dialog = page.getByRole('dialog', { name: 'Search' });
  await expect(dialog).toBeVisible();
  await dialog.getByRole('textbox').fill(query);
  // The search is debounced by 90ms; wait for rows rather than for a clock.
  await expect(dialog.getByRole('option').first()).toBeVisible();
  return dialog;
}

test('the arrow keys keep the highlighted result on screen', async ({ page }) => {
  const dialog = await openPalette(page, 'the');
  const list = dialog.getByRole('listbox', { name: 'Results' });
  const options = dialog.getByRole('option');
  const total = await options.count();
  expect(total, 'not enough results to scroll').toBeGreaterThan(8);

  /** Whether the selected row is inside the scroller's own viewport. */
  const selectedOnScreen = async () => {
    const box = await list.boundingBox();
    const row = await dialog.locator('[role="option"][aria-selected="true"]').boundingBox();
    expect(row, 'nothing is selected at all').not.toBeNull();
    return row!.y >= box!.y - 1 && row!.y + row!.height <= box!.y + box!.height + 1;
  };

  // Walk the whole list. Against the old code the highlight leaves the viewport as soon
  // as the rows run past the fold and never comes back.
  for (let i = 1; i < total; i += 1) {
    await page.keyboard.press('ArrowDown');
    expect(await selectedOnScreen(), `row ${i + 1} of ${total} is off screen`).toBe(true);
  }
  // And back up again.
  for (let i = total - 1; i > 0; i -= 1) {
    await page.keyboard.press('ArrowUp');
    expect(await selectedOnScreen(), `row ${i} of ${total} is off screen`).toBe(true);
  }
});

test('the input says which result is highlighted', async ({ page }) => {
  const dialog = await openPalette(page, 'the');
  const input = dialog.getByRole('textbox');

  const first = await input.getAttribute('aria-activedescendant');
  expect(first, 'the input points at no result').not.toBeNull();
  await expect(page.locator(`#${first}`)).toHaveAttribute('aria-selected', 'true');

  await page.keyboard.press('ArrowDown');
  const second = await input.getAttribute('aria-activedescendant');
  expect(second, 'Down changed nothing a screen reader could hear').not.toBe(first);
  await expect(page.locator(`#${second}`)).toHaveAttribute('aria-selected', 'true');
  // Exactly one row is selected, which is what a listbox means.
  await expect(dialog.locator('[role="option"][aria-selected="true"]')).toHaveCount(1);
});

test('a new query puts the highlight back at the top', async ({ page }) => {
  const dialog = await openPalette(page, 'the');
  const input = dialog.getByRole('textbox');

  await page.keyboard.press('ArrowDown');
  await page.keyboard.press('ArrowDown');
  expect(await dialog.getByRole('textbox').getAttribute('aria-activedescendant'))
    .not.toBe('palette-hit-0');

  // `setCursor(0)` ran only when the *number* of hits changed, so a different query
  // returning the same count left the highlight on row N of another list entirely.
  await input.fill('a');
  await expect(dialog.getByRole('option').first()).toBeVisible();
  await expect
    .poll(() => dialog.getByRole('textbox').getAttribute('aria-activedescendant'))
    .toBe('palette-hit-0');
});
