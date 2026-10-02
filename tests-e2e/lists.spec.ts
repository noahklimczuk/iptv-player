/**
 * My List and the thumbs-up, which did nothing at all.
 *
 * `<IconButton icon="plus" label="Add to My List" onClick={(e) => e.stopPropagation()} />`
 * — the entire handler was stopping the click reaching the card behind it. The same
 * button was on the rail cards, the hero billboard and the detail modal, and the host had
 * `mylist.toggle` implemented, registered and unit-tested the whole time.
 *
 * The thumbs-up had nothing behind it at all. It does now, and what it writes is the flag
 * `repo::recommend::history` already reads for its favourite boost — which, for films and
 * shows, nothing in the app had ever set.
 */
import { expect, test, type Page } from '@playwright/test';

const SHOTS = 'screenshots';

/** The card for a title that is not already on the list, expanded and ready. */
async function expandableCard(page: Page) {
  const rail = page.getByRole('region', { name: 'Recently Added' });
  const card = rail.getByTestId('catalog-card').first();
  await card.hover();
  await expect(card.getByRole('button', { name: /My List$/ })).toBeVisible();
  return card;
}

test('a card can be added to My List, and the rail picks it up', async ({ page }) => {
  await page.goto('/#/');
  await expect(page.getByRole('region', { name: 'Featured' })).toBeVisible();

  const card = await expandableCard(page);
  const title = (await card.getByRole('button').first().getAttribute('aria-label'))!
    .replace(/,\s*\d{4}$/, '');
  const add = card.getByRole('button', { name: `Add ${title} to My List` });
  await expect(add).toHaveAttribute('aria-pressed', 'false');
  await add.click();

  // The button says so immediately, without waiting for the round trip.
  await expect(
    card.getByRole('button', { name: `Remove ${title} from My List` }),
  ).toHaveAttribute('aria-pressed', 'true');

  // And the rail the host builds has it, which is the thing that used to not happen.
  await expect(
    page
      .getByRole('region', { name: 'My List' })
      .getByRole('button', { name: new RegExp(`^${title.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}`) }),
  ).toHaveCount(1, { timeout: 10_000 });

  await page.screenshot({ path: `${SHOTS}/51-my-list.png` });
});

test('the state is the same button wherever it is drawn', async ({ page }) => {
  await page.goto('/#/');
  await expect(page.getByRole('region', { name: 'Featured' })).toBeVisible();

  // The hero's own button — the most prominent one in the app, and the one with no
  // handler at all.
  const hero = page.getByRole('region', { name: 'Featured' });
  const heroButton = hero.getByRole('button', { name: 'My List' });
  const wasPressed = await heroButton.getAttribute('aria-pressed');
  await heroButton.click();
  await expect(heroButton).not.toHaveAttribute('aria-pressed', wasPressed!);

  // Opening the same title's detail shows the same state, because both read one store.
  await hero.getByRole('button', { name: 'More Info' }).click();
  const dialog = page.getByRole('dialog');
  await expect(dialog).toBeVisible();
  const inModal = dialog.getByRole('button', { name: /My List$/ });
  await expect(inModal).toHaveAttribute('aria-pressed', wasPressed === 'true' ? 'false' : 'true');
});

test('liking a title is remembered, and is not the same as My List', async ({ page }) => {
  // Through the detail modal rather than a hovered card: the panel this lives in on a
  // card is only there while the pointer is, which makes a two-step assertion about it
  // a test of Playwright's mouse rather than of the feature.
  await page.goto('/#/');
  const hero = page.getByRole('region', { name: 'Featured' });
  await expect(hero).toBeVisible();
  await hero.getByRole('button', { name: 'More Info' }).click();

  const dialog = page.getByRole('dialog');
  await expect(dialog).toBeVisible();
  const title = await dialog.getByRole('heading', { level: 1 }).innerText();

  const like = dialog.getByRole('button', { name: `I like ${title}` });
  await expect(like).toHaveAttribute('aria-pressed', 'false');
  await like.click();
  await expect(
    dialog.getByRole('button', { name: `Undo liking ${title}` }),
  ).toHaveAttribute('aria-pressed', 'true');

  // Liking is "more like this", not "watch later": it must not put the title on a list.
  await expect(dialog.getByRole('button', { name: `Add ${title} to My List` })).toBeVisible();

  // And it survives leaving the screen, because the host was told rather than a flag
  // being flipped in a component that is about to unmount.
  await page.keyboard.press('Escape');
  await page.goto('/#/movies');
  await page.goto('/#/');
  await expect(page.getByRole('region', { name: 'Featured' })).toBeVisible();
  await page.getByRole('region', { name: 'Featured' })
    .getByRole('button', { name: 'More Info' }).click();
  await expect(
    page.getByRole('dialog').getByRole('button', { name: `Undo liking ${title}` }),
  ).toHaveAttribute('aria-pressed', 'true');
});
