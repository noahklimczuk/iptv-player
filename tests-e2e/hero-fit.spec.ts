/**
 * The billboard's text is never cut through the middle of a line.
 *
 * Reported with a screenshot: "Beyond the Headlines: Black Girl Missing" with its
 * second line sliced in half, and half a line of synopsis under it. The column is
 * bottom-aligned in a fixed-height box with `overflow: hidden`, and flex items shrink,
 * so a long title and synopsis were each squeezed below their own height and clipped.
 *
 * The invariant, for every featured title at several window sizes: each block of text
 * sits wholly inside the box -- the synopsis gives up whole lines instead.
 */
import { expect, test, type Page } from '@playwright/test';

const LONG_TITLE = 'Beyond the Headlines: Black Girl Missing';
const LONG_OVERVIEW =
  'Two terrifying stories of young black women who go missing and the families who battle ' +
  'a system stacked against them. With the help of the media, they fight to bring their ' +
  'daughters home, and to hold to account everyone who looked away.';

async function overflowingBlocks(page: Page): Promise<string[]> {
  return page.evaluate(() => {
    const box = document.querySelector('.hero-text') as HTMLElement | null;
    if (!box) return ['no billboard'];
    const b = box.getBoundingClientRect();
    return Array.from(box.children)
      .filter((c) => (c as HTMLElement).offsetParent !== null)
      .filter((c) => {
        const r = c.getBoundingClientRect();
        return r.top < b.top - 0.5 || r.bottom > b.bottom + 0.5;
      })
      .map((c) => `${c.tagName}: ${(c as HTMLElement).innerText.slice(0, 40)}`);
  });
}

for (const [w, h] of [[1130, 760], [1280, 720], [1440, 900], [1920, 1080]] as const) {
  test(`nothing in the billboard is cut off at ${w}x${h}`, async ({ page }) => {
    await page.setViewportSize({ width: w, height: h });
    await page.goto('/#/');
    const hero = page.getByRole('region', { name: 'Featured' });
    await expect(hero).toBeVisible();

    const dots = hero.getByRole('button', { name: /^Show / });
    const n = Math.max(1, await dots.count());
    for (let i = 0; i < n; i += 1) {
      if (i > 0) await dots.nth(i).click();
      // The reported title and synopsis, the longest shape a real library gives it.
      await page.evaluate(({ t, o }) => {
        const h1 = document.querySelector('.hero-text h1');
        const p = document.querySelector('.hero-text .hero-overview');
        if (h1) h1.textContent = t;
        if (p) p.textContent = o;
        window.dispatchEvent(new Event('resize'));
      }, { t: LONG_TITLE, o: LONG_OVERVIEW });
      await page.waitForTimeout(150);
      expect(await overflowingBlocks(page), `title ${i + 1}`).toEqual([]);
    }
  });
}
