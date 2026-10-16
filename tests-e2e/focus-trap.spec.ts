/**
 * `aria-modal="true"` has to be true.
 *
 * Three dialogs asserted it — the detail modal, the command palette and the mosaic save
 * dialog — and none held focus: no trap, no initial focus, no restoration. Measured in
 * Chromium at the time, opening the detail modal from a rail card:
 *
 * ```text
 * focus right after opening: DIV[The Silent Tide, 2019] inDialog=false
 * of 12 tabs, landed outside the dialog: 12
 * focus after Escape: BUTTON[Add [SPANISH] La Casa del Lago to My List]
 * ```
 *
 * Every Tab press landed on a control behind the scrim, including Play buttons on other
 * titles — reachable by keyboard, and they start playback with a modal over them. After
 * Escape, focus was wherever the walk ended rather than on the card that opened the
 * dialog, so a keyboard user loses their place in the rail (R-102).
 */
import { expect, test, type Page } from '@playwright/test';

/** Tab `n` times, reporting how often focus landed outside the dialog. */
async function walk(page: Page, n: number) {
  const outside: string[] = [];
  for (let i = 0; i < n; i += 1) {
    await page.keyboard.press('Tab');
    const where = await page.evaluate(() => {
      const el = document.activeElement as HTMLElement | null;
      const dialog = document.querySelector('[role="dialog"]');
      return {
        inDialog: !!el && !!dialog && dialog.contains(el),
        label: `${el?.tagName}[${el?.getAttribute('aria-label') ?? el?.textContent?.slice(0, 30) ?? ''}]`,
      };
    });
    if (!where.inDialog) outside.push(where.label);
  }
  return outside;
}

test('the detail modal keeps the keyboard inside it, and gives it back', async ({ page }) => {
  await page.goto('/#/movies');
  await expect(page.getByRole('heading', { name: 'Movies' })).toBeVisible();

  // Open it from a card, by keyboard, so there is a real opener to return to.
  const card = page.getByTestId('catalog-card').first();
  const opener = card.getByRole('button').first();
  await opener.focus();
  const openerLabel = await opener.getAttribute('aria-label');
  await page.keyboard.press('Enter');

  const dialog = page.getByRole('dialog');
  await expect(dialog).toBeVisible();

  // Twelve presses, which is what the finding measured.
  const escaped = await walk(page, 12);
  expect(escaped, `focus left the dialog ${escaped.length} times: ${escaped.join(', ')}`)
    .toEqual([]);

  // Shift+Tab wraps the other way rather than stepping out of the top.
  for (let i = 0; i < 4; i += 1) await page.keyboard.press('Shift+Tab');
  expect(await page.evaluate(() => {
    const el = document.activeElement;
    const d = document.querySelector('[role="dialog"]');
    return !!el && !!d && d.contains(el);
  })).toBe(true);

  // And Escape puts it back where it came from.
  await page.keyboard.press('Escape');
  await expect(page.getByRole('dialog')).toHaveCount(0);
  await expect(page.locator(`[aria-label="${openerLabel}"]`)).toBeFocused();
});

test('the command palette keeps the keyboard inside it', async ({ page }) => {
  await page.goto('/#/');
  await expect(page.getByRole('region', { name: 'Featured' })).toBeVisible();
  await page.keyboard.press('Control+k');

  const dialog = page.getByRole('dialog', { name: 'Search' });
  await expect(dialog).toBeVisible();
  // The input keeps the initial focus, because that is where typing goes.
  await expect(dialog.getByRole('textbox')).toBeFocused();

  // Before typing, which is the state the finding measured: with no results there is
  // nothing inside the dialog to tab to, so focus went straight out to the nav rail —
  // "8 of 8 outside the dialog". A trap has to hold even when the dialog is empty.
  const empty = await walk(page, 8);
  expect(empty, `focus left the empty palette ${empty.length} times: ${empty.join(', ')}`)
    .toEqual([]);

  // And with results, where the rows themselves are the tab stops.
  await dialog.getByRole('textbox').fill('the');
  await expect(dialog.getByTestId('palette-group-movies')).toBeVisible();
  const withHits = await walk(page, 12);
  expect(
    withHits,
    `focus left the palette ${withHits.length} times: ${withHits.join(', ')}`,
  ).toEqual([]);
});
