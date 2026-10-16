/**
 * The Appearance panel's four choices, after the window has been closed.
 *
 * `useUi` was a plain `create` with no persistence and a hardcoded initial state, and
 * every handler in the panel wrote only to the in-memory store. So picking the OLED
 * theme, or TV density, lasted until the app closed. Density is the one that matters:
 * its own hint says "TV mode enlarges everything for a 10-foot UI", which is what the
 * app is for, so anybody running Aurora on a television set it again on every launch.
 *
 * Everything else in Settings goes to the host and persists, which made this the one
 * panel that did not (R-87).
 */
import { expect, test, type Page } from '@playwright/test';

/** What `<html>` is actually carrying, which is what the stylesheets read. */
function domState(page: Page) {
  return page.evaluate(() => ({
    theme: document.documentElement.dataset.theme ?? null,
    density: document.documentElement.dataset.density ?? null,
    motion: document.documentElement.dataset.motion ?? null,
  }));
}

async function openAppearance(page: Page) {
  await page.goto('/#/settings');
  await expect(page.getByRole('heading', { name: 'Appearance' })).toBeVisible();
}

test('the appearance settings are still set after a restart', async ({ page }) => {
  await openAppearance(page);
  await page.getByRole('button', { name: 'TV', exact: true }).click();
  await page.getByRole('button', { name: 'oled', exact: true }).click();

  // Applied now.
  expect(await domState(page)).toMatchObject({ theme: 'oled', density: 'tv' });

  // And after the window has gone and come back, which is the whole finding.
  await page.reload();
  await expect(page.getByRole('heading', { name: 'Appearance' })).toBeVisible();
  expect(await domState(page)).toMatchObject({ theme: 'oled', density: 'tv' });

  // The panel agrees with the document: the chosen buttons are the primary ones.
  await expect(page.getByRole('button', { name: 'TV', exact: true }))
    .toHaveAttribute('data-variant', 'primary');
  await expect(page.getByRole('button', { name: 'oled', exact: true }))
    .toHaveAttribute('data-variant', 'primary');
});

test('a fresh window carries the appearance data attributes before anything is touched',
  async ({ page }) => {
    await page.goto('/#/');
    await expect(page.getByRole('region', { name: 'Featured' })).toBeVisible();

    // `applyDom` used to run only from a setter, so these two were absent entirely on a
    // fresh window — which is unnoticeable until a stored choice exists and the store
    // says `tv` while the document says nothing.
    const dom = await domState(page);
    expect(dom.density, 'no data-density on a fresh window').not.toBeNull();
    expect(dom.motion, 'no data-motion on a fresh window').not.toBeNull();
    expect(dom.theme).toBe('dark');
  });

test('the toggles persist too, and a corrupt stored value is ignored', async ({ page }) => {
  await openAppearance(page);
  const animations = page.getByRole('switch', { name: 'Animations' });
  const was = await animations.getAttribute('aria-checked');
  await animations.click();
  await expect(animations).not.toHaveAttribute('aria-checked', was!);

  await page.reload();
  await expect(page.getByRole('heading', { name: 'Appearance' })).toBeVisible();
  await expect(page.getByRole('switch', { name: 'Animations' }))
    .not.toHaveAttribute('aria-checked', was!);

  // Whatever is in that key is a string somebody could have edited. An unknown theme
  // would go into `data-theme`, match no stylesheet, and leave the app unstyled.
  await page.evaluate(() => {
    window.localStorage.setItem(
      'aurora.appearance',
      JSON.stringify({ theme: 'neon', density: 7, animations: 'yes' }),
    );
  });
  await page.reload();
  await expect(page.getByRole('heading', { name: 'Appearance' })).toBeVisible();
  expect(await domState(page)).toMatchObject({ theme: 'dark', density: 'desktop' });
});
