/**
 * The floating navigation pill (replacing the left rail).
 *
 * The look is a matter of taste and not something to assert. What these pin down is the
 * part that is easy to get wrong when navigation stops being part of the layout and
 * starts floating over it:
 *
 * - it is *over* the content rather than beside it, and centred;
 * - the content pane keeps room for it, so the last row of a grid is reachable instead
 *   of parked underneath;
 * - it gets out of the way of video, exactly as the rail did;
 * - and it is still a `navigation` landmark full of links, because a remote control and
 *   the rest of the suite both depend on that.
 */
import { expect, test, type Page } from '@playwright/test';

const SHOTS = 'screenshots';

const pill = (page: Page) => page.getByTestId('nav-pill');

test('the nav floats near the bottom, centred over the content', async ({ page }) => {
  await page.goto('/#/');
  await expect(page.getByRole('region', { name: 'Featured' })).toBeVisible();
  await expect(pill(page)).toBeVisible();

  const box = (await pill(page).boundingBox())!;
  const viewport = page.viewportSize()!;

  // Near the bottom, not at it: a pill flush with the edge is a bar.
  expect(box.y + box.height).toBeLessThan(viewport.height);
  expect(viewport.height - (box.y + box.height)).toBeLessThan(80);
  expect(box.y).toBeGreaterThan(viewport.height * 0.6);

  // Horizontally centred, within a pixel or two of rounding.
  const centre = box.x + box.width / 2;
  expect(Math.abs(centre - viewport.width / 2)).toBeLessThan(3);

  // Fully rounded, which is the whole visual idea.
  const radius = await pill(page).evaluate((el) => getComputedStyle(el).borderRadius);
  expect(radius).not.toBe('0px');

  await page.screenshot({ path: `${SHOTS}/nav-pill.png` });
});

test('the content pane keeps room so nothing hides under the pill', async ({ page }) => {
  await page.goto('/#/movies');
  await expect(page.getByRole('heading', { name: 'Movies' })).toBeVisible();
  await page.waitForTimeout(900);

  const padding = await page.evaluate(() => {
    const pane = document.querySelector('main') as HTMLElement;
    return {
      paddingBottom: parseFloat(getComputedStyle(pane).paddingBottom),
      scrollPaddingBottom: getComputedStyle(pane).scrollPaddingBottom,
    };
  });
  const box = (await pill(page).boundingBox())!;

  // The reserved space has to actually cover the pill, or the last row of the grid is
  // behind it and cannot be clicked.
  expect(padding.paddingBottom).toBeGreaterThanOrEqual(box.height);
  // And keyboard navigation scrolls a focused card into view — "in view" has to mean
  // "not under the nav", which is what scroll padding is for.
  expect(padding.scrollPaddingBottom).not.toBe('0px');

  // Deliberately *not* asserted here: that the last card on screen sits above the pill
  // after scrolling to the end. The browse grid loads more as it is scrolled, so there
  // is always another row below — the first version of this test scrolled to the bottom,
  // found a card at y=2736, and was measuring lazy loading rather than layout. The
  // padding is the contract; whether a particular card is under the nav at a particular
  // scroll position is not.
});

test('the pill gets out of the way of video', async ({ page }) => {
  // `?video` composes as though libmpv were rendering behind the page, which is the only
  // way a browser can reach this state: `showingPicture` requires a real video surface,
  // so without the flag the chrome stays up — correctly, since hiding it over nothing
  // would leave a hole through to the desktop.
  await page.goto('/?video#/live');
  await expect(page.getByRole('heading', { name: 'Live TV' })).toBeVisible();
  await expect(pill(page)).toBeVisible();

  await page.getByRole('button', { name: 'Watch Meridian News' }).first().click();
  await expect(page.getByRole('button', { name: 'Pause' })).toBeVisible();

  // Same rule the rail followed: while there is a picture behind the page, the chrome
  // is not drawn over it.
  await expect(pill(page)).toBeHidden();
});

test('it is still a navigation landmark of links', async ({ page }) => {
  await page.goto('/#/');
  const nav = page.getByRole('navigation', { name: 'Main' });
  await expect(nav).toBeVisible();

  // Every destination, reachable by its name — which is what a remote control and the
  // rest of this suite both use.
  for (const label of ['Home', 'Live TV', 'Guide', 'Movies', 'Series', 'Settings']) {
    await expect(nav.getByRole('link', { name: label })).toBeVisible();
  }

  await nav.getByRole('link', { name: 'Guide' }).click();
  await expect(page.getByRole('heading', { name: 'TV Guide' })).toBeVisible();

  // The current destination is marked, not just coloured differently by luck.
  await expect(nav.getByRole('link', { name: 'Guide' })).toHaveAttribute(
    'aria-current',
    'page',
  );
});

/**
 * How visible it is, as numbers -- now that it is glass.
 *
 * Reported first as "make the tab bar more visible": a pill filled at 82% with a 10%
 * border and faint labels disappeared over a bright rail. Those numbers were pinned
 * here as an opaque fill and a strong border. Then glass was asked for, which is a
 * translucent fill and a light edge by definition -- so the guarantee moves to what the
 * complaint was actually about, and gets stricter rather than looser:
 *
 *   - it is frosted, not merely translucent: the page behind is blurred, which is what
 *     keeps whatever is under it from competing with the labels;
 *   - a resting label reaches WCAG AA (4.5:1) against the glass laid over a pure white
 *     page and over a pure black one -- the two extremes of anything that can blur
 *     behind it -- in every theme that uses glass;
 *   - it is lifted off the page by a shadow and edged by a specular highlight.
 *
 * The contrast theme stays solid, and is held to the same 4.5:1 on its own fill.
 */
for (const theme of ['dark', 'oled', 'light', 'contrast'] as const) {
  test(`the pill is findable and its labels readable on any page (${theme})`, async ({ page }) => {
    await page.goto('/#/');
    await page.evaluate((t) => {
      window.localStorage.setItem('aurora.appearance', JSON.stringify({ theme: t }));
    }, theme);
    await page.reload();
    await expect(pill(page)).toBeVisible();
    await expect(page.locator('html')).toHaveAttribute('data-theme', theme);

    const look = await page.evaluate(() => {
      const el = document.querySelector('[data-testid="nav-pill"]') as HTMLElement;
      const style = getComputedStyle(el);
      const resting = Array.from(el.querySelectorAll('a')).find(
        (a) => a.getAttribute('aria-current') !== 'page',
      ) as HTMLElement;
      const parse = (c: string) => {
        const n = c.match(/[\d.]+/g)!.map(Number);
        return { r: n[0]!, g: n[1]!, b: n[2]!, a: n[3] ?? 1 };
      };
      return {
        fill: parse(style.backgroundColor),
        filter: style.backdropFilter || (style as unknown as { webkitBackdropFilter: string }).webkitBackdropFilter,
        label: parse(getComputedStyle(resting).color),
        fontSize: parseFloat(getComputedStyle(resting).fontSize),
        shadow: style.boxShadow,
      };
    });

    const channel = (v: number) => {
      const c = v / 255;
      return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
    };
    const lum = (c: { r: number; g: number; b: number }) =>
      0.2126 * channel(c.r) + 0.7152 * channel(c.g) + 0.0722 * channel(c.b);
    const over = (bg: number) => ({
      r: look.fill.r * look.fill.a + bg * (1 - look.fill.a),
      g: look.fill.g * look.fill.a + bg * (1 - look.fill.a),
      b: look.fill.b * look.fill.a + bg * (1 - look.fill.a),
    });
    const ratio = (x: { r: number; g: number; b: number }) => {
      const a = lum(look.label);
      const b = lum(x);
      return (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);
    };

    if (theme === 'contrast') {
      expect(look.fill.a, 'the contrast theme is solid, not glass').toBe(1);
    } else {
      expect(look.filter, 'glass is frosted, not merely see-through').toContain('blur');
    }
    for (const [name, bg] of [['white', 255], ['black', 0]] as const) {
      const r = ratio(over(bg));
      expect(r, `resting labels are ${r.toFixed(2)}:1 over a ${name} page`)
        .toBeGreaterThanOrEqual(4.5);
    }
    expect(look.fontSize).toBeGreaterThanOrEqual(11);
    expect(look.shadow).not.toBe('none');
  });
}
