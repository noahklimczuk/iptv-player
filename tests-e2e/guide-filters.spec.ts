/**
 * The guide's filters, where you are on it, and the glass transparency slider.
 *
 * Asked for, in one message: "add comprehensive filters to the guide allowing me to only
 * show certain groups of channels", "make it easier to see where I am on the guide",
 * and "I want the liquid glass to be more transparent, add a slider in the settings".
 */
import { expect, test, type Page } from '@playwright/test';

const groupChips = (page: Page) =>
  page.getByRole('group', { name: 'Category' }).getByTestId('guide-group');

async function chipNames(page: Page) {
  return (await groupChips(page).allInnerTexts()).map((t) => t.replace(/\s*\d[\d,]*$/, '').trim());
}

async function groupsOnScreen(page: Page) {
  // Every channel row's group, from the host list the mock serves.
  return page.getByTestId('guide-channel').count();
}

test('several groups can be shown at once, and the choice is kept', async ({ page }) => {
  await page.goto('/#/guide');
  await expect(groupChips(page).first()).toBeVisible();
  const all = await groupsOnScreen(page);
  const [a, b] = await chipNames(page);

  await groupChips(page).nth(0).click();
  await expect(groupChips(page).nth(0)).toHaveAttribute('aria-pressed', 'true');
  const one = await groupsOnScreen(page);
  await groupChips(page).nth(1).click();
  // Both on: the second adds to the first rather than replacing it.
  await expect(groupChips(page).nth(0)).toHaveAttribute('aria-pressed', 'true');
  await expect(groupChips(page).nth(1)).toHaveAttribute('aria-pressed', 'true');
  await expect.poll(() => groupsOnScreen(page)).toBeGreaterThan(one);
  expect(await groupsOnScreen(page)).toBeLessThanOrEqual(all);

  await expect(page.getByTestId('guide-filters')).toContainText('1');
  await page.reload();
  await expect(page.getByRole('group', { name: 'Category' })
    .getByRole('button', { name: new RegExp(`^${a}`) })).toHaveAttribute('aria-pressed', 'true');
  await expect(page.getByRole('group', { name: 'Category' })
    .getByRole('button', { name: new RegExp(`^${b}`) })).toHaveAttribute('aria-pressed', 'true');

  await page.getByRole('group', { name: 'Category' })
    .getByRole('button', { name: 'All channels' }).click();
  await expect.poll(() => groupsOnScreen(page)).toBe(all);
});

test('the filter panel hides groups, searches them, and filters channels', async ({ page }) => {
  await page.goto('/#/guide');
  await expect(groupChips(page).first()).toBeVisible();
  const [first] = await chipNames(page);

  await page.getByTestId('guide-filters').click();
  const panel = page.getByRole('dialog', { name: 'Guide filters' });
  await expect(panel).toBeVisible();
  // The rows are virtualised, so the panel's count is what says how many are left.
  const shown = async () =>
    Number((await panel.getByTestId('guide-filter-shown').innerText()).replace(/\D/g, ''));
  const all = await shown();

  await panel.getByRole('radio', { name: 'Hide' }).click();
  await panel.getByLabel('Search groups').fill(first!.slice(0, 4));
  await panel.getByRole('checkbox').first().check();
  await expect.poll(shown).toBeLessThan(all);

  await panel.getByRole('switch', { name: 'Catch-up only' }).click();
  await expect(panel.getByRole('switch', { name: 'Catch-up only' }))
    .toHaveAttribute('aria-checked', 'true');

  await panel.getByRole('button', { name: 'Reset all' }).click();
  await expect.poll(shown).toBe(all);
  await page.keyboard.press('Escape');
  await expect(panel).toBeHidden();
});

test('where you are is marked across the row, on the programme, and in the time bar', async ({ page }) => {
  await page.goto('/#/guide');
  await expect(page.getByTestId('guide-row').first()).toBeVisible();
  await page.locator('body').click({ position: { x: 5, y: 5 } });

  await page.keyboard.press('ArrowDown');
  await expect(page.locator('[data-testid="guide-row"][data-current="true"]')).toHaveCount(1);
  await page.keyboard.press('ArrowRight');
  const picked = page.locator('[data-testid="programme"][aria-pressed="true"]');
  await expect(picked).toHaveCount(1);
  await expect(page.getByTestId('guide-time-mark')).toBeVisible();
  await expect(page.getByTestId('guide-time-band')).toHaveCount(1);
  const ring = await picked.evaluate((el) => getComputedStyle(el).boxShadow);
  expect(ring).not.toBe('none');

  // Down keeps the time: a programme stays selected on the next channel when it has
  // one at that moment, instead of the selection disappearing.
  const before = await page.getByTestId('guide-time-mark').boundingBox();
  await page.keyboard.press('ArrowDown');
  const still = await page.locator('[data-testid="programme"][aria-pressed="true"]').count();
  if (still === 1) {
    const after = (await page.getByTestId('guide-time-mark').boundingBox())!;
    // The mark covers the moment it was on, so it overlaps where it was.
    expect(after.x).toBeLessThan(before!.x + before!.width);
    expect(after.x + after.width).toBeGreaterThan(before!.x);
  }
});

test('the glass transparency slider changes the glass and is kept', async ({ page }) => {
  await page.goto('/#/settings');
  const slider = page.getByRole('slider', { name: 'Glass transparency' });
  await expect(slider).toBeVisible();
  await expect(slider).toHaveValue('50');

  const tintAt = () =>
    page.getByTestId('nav-pill').evaluate((el) => {
      const n = getComputedStyle(el).backgroundColor.match(/[\d.]+/g)!.map(Number);
      return n[3] ?? 1;
    });
  await slider.fill('0');
  const frosted = await tintAt();
  await slider.fill('100');
  const clear = await tintAt();
  expect(clear, 'clearer glass is a thinner tint').toBeLessThan(frosted);
  expect(await page.evaluate(() =>
    getComputedStyle(document.documentElement).getPropertyValue('--glass-clarity').trim(),
  )).toBe('1');

  await page.reload();
  await expect(page.getByRole('slider', { name: 'Glass transparency' })).toHaveValue('100');
});
