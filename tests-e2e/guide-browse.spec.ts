/**
 * Browsing the guide for something to watch.
 *
 * More than half of a real line has no schedule anywhere -- event slots, idle slots and
 * 24/7 loops -- and the guide drew each of them as a dashed "No guide data" strip. It
 * now reads what the channel's name says is on, marked as such, and a switch narrows the
 * guide to channels with real listings.
 */
import { expect, test } from '@playwright/test';

test('a channel with no guide says what its name says is on', async ({ page }) => {
  await page.goto('/#/guide');
  await page.getByRole('group', { name: 'Category' }).getByRole('button', { name: /^Events/ }).click();

  const rows = page.getByTestId('guide-from-name');
  await expect(rows).toHaveCount(3);
  await expect(rows.filter({ hasText: 'Hockey: Des Moines Buccaneers vs Fargo Force' }))
    .toHaveAttribute('data-kind', 'event');
  await expect(rows.filter({ hasText: '7PM ET' })).toHaveAttribute('data-kind', 'event');
  await expect(rows.filter({ hasText: 'Nothing scheduled on this event channel' }))
    .toHaveAttribute('data-kind', 'idle');
  // Never mistaken for a schedule: each one says where it came from, and none of them
  // is a programme that could be selected, reminded or recorded.
  await expect(rows.first()).toContainText('from channel name');
  await expect(page.getByTestId('programme')).toHaveCount(0);
});

test('"With listings" narrows the guide to channels with a schedule', async ({ page }) => {
  await page.goto('/#/guide');
  const unguided = page.getByTestId('guide-channel').filter({ hasText: 'Kardashians' });
  const toggle = page.getByRole('switch', { name: 'Only channels with listings' });

  await page.getByRole('group', { name: 'Category' }).getByRole('button', { name: /^24\/7/ }).click();
  await expect(unguided).toHaveCount(1);

  await toggle.click();
  await expect(toggle).toHaveAttribute('aria-checked', 'true');
  await expect(unguided).toHaveCount(0);
  await expect(page.getByText('No channels with listings here')).toBeVisible();

  // Remembered for the next visit, and undone as easily.
  await page.reload();
  await expect(page.getByRole('switch', { name: 'Only channels with listings' }))
    .toHaveAttribute('aria-checked', 'true');
  await page.getByRole('switch', { name: 'Only channels with listings' }).click();
});
