/**
 * Trailers, which did not exist.
 *
 * The hero billboard printed "NOW PLAYING TRAILER" over a slowly zooming backdrop and
 * offered a speaker button bound to a state variable that no audio obeyed. The rail cards
 * said "Preview playing" after a dwell. Nothing fetched a trailer and nothing played one,
 * and both claims were made about every title regardless.
 *
 * These assert the embed is actually mounted, and — just as important — that neither
 * claim is made about a title with no trailer to play. Shows all carry one in the
 * fixtures and only every third film does, so a journey can pick a card and know which
 * case it is looking at.
 */
import { expect, test, type Page } from '@playwright/test';

const SHOTS = 'screenshots';

async function settle(page: Page, ms = 700) {
  await page.waitForLoadState('networkidle').catch(() => {});
  await page.waitForTimeout(ms);
}

/** The embed address, so the assertions are about YouTube and not about any iframe. */
const EMBED = /^https:\/\/www\.youtube-nocookie\.com\/embed\//;

test('hovering a show plays its trailer in a popup @motion', async ({ page }) => {
  await page.goto('/#/series');
  await expect(page.getByRole('heading', { name: 'Series' })).toBeVisible();
  const cards = page.getByTestId('catalog-card');
  await expect(cards.first()).toBeVisible();

  await cards.first().hover();
  // The dwell before a preview starts is 700ms.
  //
  // In a popup over the page, not inside the tile. It used to play over the poster --
  // 16:9 inside a 2:3 card, so it covered the middle band of something 150px wide:
  // enough to prove a trailer exists and too small to watch, for the cost of a real
  // video player either way.
  const popup = page.getByTestId('trailer-popup');
  await expect(popup).toBeVisible({ timeout: 5000 });
  const frame = popup.getByTestId('trailer-frame');
  await expect(frame).toBeVisible({ timeout: 5000 });

  // Bigger than the card it came from, which is the whole point of the change.
  const card = await cards.first().boundingBox();
  const playing = await frame.boundingBox();
  expect(playing!.width).toBeGreaterThan(card!.width * 2);
  await expect(frame).toHaveAttribute('src', EMBED);
  // Muted, because muted is the only autoplay an engine allows.
  await expect(frame).toHaveAttribute('src', /[?&]mute=1/);
  await expect(frame).toHaveAttribute('src', /[?&]autoplay=1/);
  // And the label is only there because the video is.
  await expect(cards.first().getByText('Preview playing')).toBeVisible();

  // Moving away closes it, which is how a hover-driven thing has to end.
  await page.mouse.move(5, 5);
  await expect(popup).toHaveCount(0);

  await page.screenshot({ path: `${SHOTS}/48-card-trailer.png` });
});

test('a film with no trailer neither plays one nor claims to', async ({ page }) => {
  await page.goto('/#/movies');
  await expect(page.getByRole('heading', { name: 'Movies' })).toBeVisible();
  const cards = page.getByTestId('catalog-card');
  await expect(cards.first()).toBeVisible();

  // Find one the fixtures gave no trailer: two thirds of films, so this is quick.
  const n = Math.min(await cards.count(), 12);
  let found = false;
  for (let i = 0; i < n; i += 1) {
    const card = cards.nth(i);
    await card.hover();
    await page.waitForTimeout(950);
    if ((await card.getByTestId('trailer-frame').count()) > 0) continue;
    found = true;
    // The old behaviour: this label appeared on every card after the dwell, whether
    // there was anything playing or not.
    await expect(card.getByText('Preview playing')).toHaveCount(0);
    break;
  }
  expect(found, 'every film in the fixtures had a trailer').toBe(true);
});

test('the detail modal plays a trailer on request, with sound and controls', async ({ page }) => {
  await page.goto('/#/series');
  await expect(page.getByRole('heading', { name: 'Series' })).toBeVisible();
  const cards = page.getByTestId('catalog-card');
  await expect(cards.first()).toBeVisible();
  await cards.first().getByRole('button').first().click();

  const dialog = page.getByRole('dialog');
  await expect(dialog).toBeVisible();
  // Nothing is playing until asked: the modal is opened to read, and a video starting
  // under the synopsis is an interruption.
  await expect(dialog.getByTestId('trailer-frame')).toHaveCount(0);

  await dialog.getByRole('button', { name: 'Trailer', exact: true }).click();
  const frame = dialog.getByTestId('trailer-frame');
  await expect(frame).toBeVisible();
  // Here the trailer is the subject, so it has sound and YouTube's own controls.
  await expect(frame).toHaveAttribute('src', /[?&]mute=0/);
  await expect(frame).toHaveAttribute('src', /[?&]controls=1/);
  await page.screenshot({ path: `${SHOTS}/49-detail-trailer.png` });

  // And it can be dismissed back to the metadata it was covering.
  await dialog.getByRole('button', { name: 'Stop trailer' }).click();
  await expect(dialog.getByTestId('trailer-frame')).toHaveCount(0);
  await expect(dialog.getByRole('button', { name: /^(Play|Resume)/ })).toBeVisible();
});

test('turning animations off stops trailers starting by themselves @motion', async ({ page }) => {
  await page.goto('/#/settings');
  await settle(page, 400);
  // The same switch already governed hover previews; it now governs something real.
  await page.getByRole('switch', { name: /Animations/i }).click();

  await page.goto('/#/series');
  await expect(page.getByRole('heading', { name: 'Series' })).toBeVisible();
  const card = page.getByTestId('catalog-card').first();
  await expect(card).toBeVisible();
  await card.hover();
  await page.waitForTimeout(1200);
  await expect(card.getByTestId('trailer-frame')).toHaveCount(0);

  // The card still has to *work*. `toHaveCount(0)` cannot tell "the trailer was
  // correctly suppressed" from "nothing happened at all", and for a long time it was
  // the second: `enter()` returned before `setExpanded(true)` when animations were off,
  // so this test read as proof about trailers while the card had in fact stopped
  // responding to hover entirely -- no Play, no My List, no thumbs-up, no More info, for
  // every viewer with reduced motion enabled in Windows (R-115). This is the positive
  // assertion that tells the two apart.
  await expect(card.getByRole('button', { name: /^Play / })).toBeVisible();
});
