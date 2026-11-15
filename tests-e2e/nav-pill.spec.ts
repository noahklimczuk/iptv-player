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
 * How visible it is, as numbers -- now that it is Liquid Glass.
 *
 * Reported first as "make the tab bar more visible": a pill filled at 82% with faint
 * labels disappeared over a bright rail. Then glass was asked for, and then "make the
 * liquid glass identical to the iOS one" -- which is clear glass, a thin tint. Apple
 * keeps labels legible on it by compressing the brightness of what is behind (dark glass
 * dims the page, light glass lifts it) rather than by painting over it, so the fill
 * colour alone no longer says what a label sits on. This measures what is actually
 * drawn:
 *
 *   - it is frosted, not merely see-through: the page behind is blurred;
 *   - with a pure white page behind it and then a pure black one -- the two extremes of
 *     anything that can be there -- the glass is screenshotted where a resting label
 *     sits (labels hidden), and the label's colour reaches WCAG AA (4.5:1) against
 *     those pixels, in every theme;
 *   - it is lifted off the page by a shadow.
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
      const n = getComputedStyle(resting).color.match(/[\d.]+/g)!.map(Number);
      const fill = style.backgroundColor.match(/[\d.]+/g)!.map(Number);
      return {
        fillAlpha: fill[3] ?? 1,
        filter:
          style.backdropFilter ||
          (style as unknown as { webkitBackdropFilter: string }).webkitBackdropFilter,
        label: { r: n[0]!, g: n[1]!, b: n[2]! },
        fontSize: parseFloat(getComputedStyle(resting).fontSize),
        shadow: style.boxShadow,
      };
    });

    if (theme === 'contrast') {
      expect(look.fillAlpha, 'the contrast theme is solid, not glass').toBe(1);
    } else {
      expect(look.filter, 'glass is frosted, not merely see-through').toContain('blur');
    }

    const channel = (v: number) => {
      const c = v / 255;
      return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
    };
    const lum = (c: { r: number; g: number; b: number }) =>
      0.2126 * channel(c.r) + 0.7152 * channel(c.g) + 0.0722 * channel(c.b);
    const ratio = (x: { r: number; g: number; b: number }) => {
      const a = lum(look.label);
      const b = lum(x);
      return (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);
    };

    // Labels and the platter out of the way, so the shot is of the glass alone.
    await page.addStyleTag({
      content: '[data-testid="nav-pill"] a, .nav-platter { visibility: hidden !important; }',
    });
    const resting = pill(page).locator('a:not([aria-current])').first();
    for (const [name, bg] of [['white', '#ffffff'], ['black', '#000000']] as const) {
      await page.evaluate((colour) => {
        document.getElementById('behind-pill')?.remove();
        const d = document.createElement('div');
        d.id = 'behind-pill';
        d.style.cssText = `position:fixed;inset:0;z-index:49;background:${colour}`;
        document.body.appendChild(d);
      }, bg);
      await page.waitForTimeout(150);
      const box = (await resting.boundingBox())!;
      // The middle of where the label sits, clear of the rim's highlight.
      const shot = await page.screenshot({
        clip: {
          x: box.x + box.width * 0.3,
          y: box.y + box.height * 0.3,
          width: box.width * 0.4,
          height: box.height * 0.4,
        },
      });
      const under = await page.evaluate(async (b64) => {
        const img = new Image();
        img.src = `data:image/png;base64,${b64}`;
        await img.decode();
        const c = document.createElement('canvas');
        c.width = img.width;
        c.height = img.height;
        const ctx = c.getContext('2d')!;
        ctx.drawImage(img, 0, 0);
        const d = ctx.getImageData(0, 0, c.width, c.height).data;
        let r = 0;
        let g = 0;
        let b = 0;
        const px = d.length / 4;
        for (let i = 0; i < d.length; i += 4) {
          r += d[i]!;
          g += d[i + 1]!;
          b += d[i + 2]!;
        }
        return { r: r / px, g: g / px, b: b / px };
      }, shot.toString('base64'));
      const r = ratio(under);
      const drawn = `rgb(${Math.round(under.r)} ${Math.round(under.g)} ${Math.round(under.b)})`;
      expect(r, `resting labels are ${r.toFixed(2)}:1 over a ${name} page (glass drawn as ${drawn})`)
        .toBeGreaterThanOrEqual(4.5);
    }
    expect(look.fontSize).toBeGreaterThanOrEqual(11);
    expect(look.shadow).not.toBe('none');
  });
}

test('the current tab sits on a platter that moves to the tab chosen', async ({ page }) => {
  await page.goto('/#/');
  await expect(pill(page)).toBeVisible();
  const platter = pill(page).locator('.nav-platter');
  const home = pill(page).getByRole('link', { name: 'Home' });
  const guide = pill(page).getByRole('link', { name: 'Guide' });

  const offset = async (tab: typeof home) => {
    const t = (await tab.boundingBox())!;
    const p = (await platter.boundingBox())!;
    return Math.abs(p.x + p.width / 2 - (t.x + t.width / 2));
  };
  await expect.poll(() => offset(home)).toBeLessThan(2);

  await guide.click();
  await expect(guide).toHaveAttribute('aria-current', 'page');
  // It slides there on a spring, and comes to rest under the new tab.
  await expect.poll(() => offset(guide), { timeout: 3000 }).toBeLessThan(2);

  // Every tab the same width, as on iOS.
  const widths = await pill(page)
    .locator('a')
    .evaluateAll((as) => as.map((a) => (a as HTMLElement).offsetWidth));
  expect(new Set(widths).size).toBe(1);
});

test('dragging the platter along the bar and letting go goes to that tab', async ({ page }) => {
  await page.goto('/#/');
  await expect(pill(page)).toBeVisible();
  const home = (await pill(page).getByRole('link', { name: 'Home' }).boundingBox())!;
  const series = (await pill(page).getByRole('link', { name: 'Series' }).boundingBox())!;
  const y = home.y + home.height / 2;

  await page.mouse.move(home.x + home.width / 2, y);
  await page.mouse.down();
  await expect(pill(page)).toHaveAttribute('data-pressed', 'true');
  await page.mouse.move(series.x + series.width / 2, y, { steps: 12 });
  await expect(pill(page)).toHaveAttribute('data-dragging', 'true');
  await page.mouse.up();

  await expect(pill(page).getByRole('link', { name: 'Series' })).toHaveAttribute(
    'aria-current',
    'page',
  );
  await expect(pill(page)).not.toHaveAttribute('data-pressed');
});

test('the glass bends what is behind it at the rim', async ({ page }) => {
  await page.goto('/#/');
  await expect(pill(page)).toBeVisible();
  // The lens is an SVG displacement map in front of the blur, sized to the bar.
  await expect
    .poll(() =>
      pill(page).evaluate((el) => getComputedStyle(el).getPropertyValue('--glass-lens').trim()),
    )
    .toMatch(/^url\(#aurora-lens-\d+\)$/);
  const filter = await pill(page).evaluate((el) => getComputedStyle(el).backdropFilter);
  expect(filter).toContain('url(');
  expect(filter).toContain('blur');
});
