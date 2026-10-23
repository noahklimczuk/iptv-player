/**
 * The keyboard the `Select` promises by declaring itself a listbox.
 *
 * `Select` replaced every native `<select>` in the app, and it carried the roles — the
 * trigger says `aria-haspopup="listbox"`, the popover is `role="listbox"`, the rows are
 * `role="option"` with `aria-selected` — while implementing none of the keyboard that
 * goes with them. A screen-reader user told this is a listbox presses Down and nothing
 * happened. Tab did work, because the rows are real buttons, which is a legitimate
 * pattern on its own but not the one being announced (R-116).
 *
 * Focus was the other half, and the same omission as R-102: opening left the focus on the
 * trigger, so the arrow keys had nowhere to arrive; closing left it nowhere at all, so
 * Tab from a closed Select started again at the top of the page.
 *
 * Driven on the guide's category filter because it has groups, a placeholder row and more
 * options than fit, which is the shape every other caller is a simplification of.
 */
import { expect, test } from '@playwright/test';

async function openCategory(page: import('@playwright/test').Page) {
  await page.goto('/#/guide');
  const trigger = page.getByTestId('select-category');
  await expect(trigger).toBeVisible();
  await trigger.click();
  const list = page.getByRole('listbox', { name: 'Category' });
  await expect(list).toBeVisible();
  return { trigger, list };
}

test('arrow keys walk the options, and say which one they are on', async ({ page }) => {
  const { list } = await openCategory(page);

  // The cursor starts on the current choice, which on arrival is the placeholder.
  const first = await list.getAttribute('aria-activedescendant');
  expect(first, 'the listbox must say where the keyboard is').toBeTruthy();

  await page.keyboard.press('ArrowDown');
  const second = await list.getAttribute('aria-activedescendant');
  expect(second).not.toBe(first);
  // And it is a row that exists, which is the half `aria-activedescendant` gets wrong
  // when it is pointed at an index rather than an id.
  await expect(page.locator(`#${second}`)).toBeVisible();
  await expect(page.locator(`#${second}`)).toHaveAttribute('role', 'option');

  await page.keyboard.press('ArrowUp');
  expect(await list.getAttribute('aria-activedescendant')).toBe(first);

  // Home and End are part of the same contract.
  await page.keyboard.press('End');
  const last = await list.getAttribute('aria-activedescendant');
  expect(last).not.toBe(first);
  await page.keyboard.press('Home');
  expect(await list.getAttribute('aria-activedescendant')).toBe(first);

  // The highlight is drawn, not only announced: a keyboard position nobody can see is
  // the same bug one row further on (R-147).
  await page.keyboard.press('ArrowDown');
  await expect(page.locator('[data-testid=select-row][data-cursored]')).toHaveCount(1);
});

test('Enter chooses the highlighted option and hands the focus back', async ({ page }) => {
  const { trigger, list } = await openCategory(page);

  await page.keyboard.press('ArrowDown');
  const target = await list.getAttribute('aria-activedescendant');
  const chosen = (await page.locator(`#${target}`).innerText()).split('\n')[0]!.trim();

  await page.keyboard.press('Enter');

  // Closed, and the choice took effect rather than only the popover going away.
  await expect(list).toBeHidden();
  await expect(trigger).toContainText(chosen);

  // The focus is back on the trigger. Without this, Tab from here restarted at the top
  // of the document, which is what R-102 found in the palette.
  await expect(trigger).toBeFocused();
});

test('Escape closes it without choosing, and still returns the focus', async ({ page }) => {
  const { trigger, list } = await openCategory(page);
  const before = await trigger.innerText();

  await page.keyboard.press('ArrowDown');
  await page.keyboard.press('Escape');

  await expect(list).toBeHidden();
  expect(await trigger.innerText(), 'Escape is a cancel, not a choice').toBe(before);
  await expect(trigger).toBeFocused();
});

test('a group heading is not announced as an option', async ({ page }) => {
  const { list } = await openCategory(page);

  // A listbox's children are options or groups. The headings are labels drawn between
  // rows, and an unlabelled `div` among the options is a stray node to anything reading
  // the roles, so they are marked presentational.
  const headings = list.getByTestId('select-group');
  if ((await headings.count()) > 0) {
    await expect(headings.first()).toHaveAttribute('role', 'presentation');
  }
  // And nothing claiming to be an option is one of them: every option is a real row.
  const options = await list.getByRole('option').count();
  const rows = await list.getByTestId('select-row').count();
  expect(options).toBe(rows);
  expect(rows).toBeGreaterThan(1);
});
