/**
 * Walking the guide with the arrow keys, and the preview following.
 *
 * Asked for: navigate the guide with the arrow keys and have the preview play by
 * itself. Up and Down belong to the volume everywhere else in the app, so the guide has
 * to take them first -- and the preview waits for the selection to rest, because this
 * line allows one connection and running down the list must not open a stream for
 * every channel passed on the way.
 */
import { expect, test, type Page } from '@playwright/test';

interface MockCall { name: string; args: { kind?: string; id?: number } }

async function plays(page: Page): Promise<MockCall[]> {
  const all = await page.evaluate(
    () => (window as unknown as { __auroraCalls: () => MockCall[] }).__auroraCalls(),
  );
  return all.filter((c) => c.name === 'player.play');
}

async function openGuide(page: Page) {
  await page.goto('/#/guide');
  await expect(page.getByTestId('guide-channel').first()).toBeVisible();
  await page.evaluate(
    () => (window as unknown as { __auroraClearCalls: () => void }).__auroraClearCalls(),
  );
}

const channelNames = (page: Page) =>
  page.getByTestId('guide-channel').evaluateAll((els) =>
    els.map((e) => e.getAttribute('data-channel-name')));

test('Down moves to the next channel and previews it once the selection rests', async ({ page }) => {
  await openGuide(page);
  const names = await channelNames(page);

  await page.keyboard.press('ArrowDown');
  const second = page.locator(`[data-testid=guide-channel][data-channel-name="${names[1]}"]`);
  // Previewing on its own, without a click.
  await expect(second).toHaveAttribute('data-live', 'true');
  const opened = await plays(page);
  expect(opened).toHaveLength(1);
  expect(opened[0]!.args.kind).toBe('live');
  // A preview is not a zap: the box names the channel, so no banner over the tab bar.
  await expect(page.getByTestId('channel-banner')).toHaveCount(0);
});

test('running down the list opens only the channel it stops on', async ({ page }) => {
  await openGuide(page);
  const names = await channelNames(page);

  for (let i = 0; i < 4; i += 1) await page.keyboard.press('ArrowDown');
  const landed = page.locator(`[data-testid=guide-channel][data-channel-name="${names[4]}"]`);
  await expect(landed).toHaveAttribute('data-live', 'true');
  await page.waitForTimeout(800);
  expect((await plays(page)).length, 'one stream, not one per channel passed').toBe(1);

  // Up comes back, and that is a second preview.
  await page.keyboard.press('ArrowUp');
  await expect(page.locator(`[data-testid=guide-channel][data-channel-name="${names[3]}"]`))
    .toHaveAttribute('data-live', 'true');
});

test('Right selects the next programme on the channel, and the arrows leave the volume alone', async ({ page }) => {
  await openGuide(page);
  const before = await page.evaluate(async () => {
    const inv = (window as unknown as {
      __auroraInvoke: (c: string, a?: unknown) => Promise<{ volume: number }>;
    }).__auroraInvoke;
    return (await inv('player.state')).volume;
  });

  await page.keyboard.press('ArrowRight');
  await page.keyboard.press('ArrowRight');
  // The info pane describes the programme the keyboard is on.
  await expect(page.getByTestId('programme').and(page.locator('[style*="var(--accent)"]')).first())
    .toBeVisible();
  await page.keyboard.press('ArrowDown');
  await page.keyboard.press('ArrowUp');

  const after = await page.evaluate(async () => {
    const inv = (window as unknown as {
      __auroraInvoke: (c: string, a?: unknown) => Promise<{ volume: number }>;
    }).__auroraInvoke;
    return (await inv('player.state')).volume;
  });
  expect(typeof before, 'the player reports a volume to compare').toBe('number');
  expect(after, 'Up and Down are the guide’s here, not the volume').toBe(before);
});
