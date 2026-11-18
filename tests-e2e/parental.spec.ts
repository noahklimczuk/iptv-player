/**
 * Parental controls, end to end (README §11): a PIN, a lock, and the PIN asked for when
 * the locked channel is played and when a guarded setting is changed.
 *
 * Every refusal comes from the mock in the host's own words (`ipc/parentalMessages.ts`),
 * so what is exercised is the real path on Windows: the command is refused, `invoke`
 * opens the PIN dialog, and the command is tried again once the PIN is right.
 */
import { expect, test, type Page } from '@playwright/test';

const SHOTS = 'screenshots';

async function invoke<T>(page: Page, cmd: string, args?: unknown): Promise<T> {
  return page.evaluate(
    ([c, a]) => (window as unknown as {
      __auroraInvoke: (c: string, a: unknown) => Promise<unknown>;
    }).__auroraInvoke(c as string, a),
    [cmd, args] as const,
  ) as Promise<T>;
}

async function typePin(page: Page, pin: string) {
  const dialog = page.getByRole('dialog', { name: 'Parental PIN' });
  for (const d of pin) await dialog.getByRole('button', { name: d, exact: true }).click();
}

async function parentalSection(page: Page, base = '/') {
  // Only the hash changes, so the mock's state survives the trip.
  await page.goto(`${base}#/settings`);
  const section = page.getByRole('region', { name: 'Parental controls' });
  await section.scrollIntoViewIfNeeded();
  await expect(section).toBeVisible();
  return section;
}

test('a locked channel shows a padlock and plays only after the parental PIN', async ({ page }) => {
  const section = await parentalSection(page);

  // No PIN yet: nothing can be locked, and the panel says why.
  await expect(section).toContainText('Set a parental PIN to lock channels');
  await section.getByRole('button', { name: 'Set a PIN' }).click();
  await section.getByLabel('New parental PIN', { exact: true }).fill('2468');
  await section.getByLabel('New parental PIN again').fill('2468');
  await section.getByRole('button', { name: 'Save PIN' }).click();
  await expect(section.getByRole('button', { name: 'Change PIN' })).toBeVisible();

  // Lock the first channel by name.
  const channels = await invoke<{ id: number; name: string }[]>(page, 'channels.list', {});
  const first = channels[0]!;
  await section.getByLabel('Find a channel to lock').fill(first.name.slice(0, 6));
  await section.getByRole('button', { name: `Lock ${first.name}` }).first().click();
  await expect(section.getByRole('list', { name: 'Locked channels and categories' }))
    .toContainText(first.name);
  await page.screenshot({ path: `${SHOTS}/parental-settings.png` });

  // Leaving Settings closes what the PIN opened.
  await page.goto('/#/live');
  const row = page.getByRole('button', { name: `Watch ${first.name}, locked` });
  await expect(row).toBeVisible();

  await row.click();
  const dialog = page.getByRole('dialog', { name: 'Parental PIN' });
  await expect(dialog).toBeVisible();
  await expect(dialog).toContainText('Enter the parental PIN to watch this');
  await page.screenshot({ path: `${SHOTS}/parental-pin.png` });

  // A wrong PIN is counted and says so; the channel does not play.
  await typePin(page, '1111');
  await expect(dialog.getByRole('alert')).toContainText('Incorrect PIN');
  expect((await invoke<{ channelId: number | null }>(page, 'player.state')).channelId)
    .not.toBe(first.id);

  // The right one plays it, with no second click.
  await typePin(page, '2468');
  await expect(dialog).toBeHidden();
  await expect.poll(async () =>
    (await invoke<{ channelId: number | null }>(page, 'player.state')).channelId).toBe(first.id);
});

test('cancelling the PIN leaves the locked channel unplayed and says why', async ({ page }) => {
  // Set up through the commands rather than the screens; a reload would lose it, since
  // the browser build keeps its library in memory, so the page is only navigated.
  await page.goto('/#/');
  await expect(page.getByRole('region', { name: 'Featured' })).toBeVisible();
  await invoke(page, 'profiles.setParental', { hideAdult: true, lockSettings: false, masterPin: '2468' });
  const channels = await invoke<{ id: number; name: string }[]>(page, 'channels.list', {});
  const first = channels[0]!;
  await invoke(page, 'parental.setLock', { kind: 'channel', value: String(first.id), locked: true });
  await invoke(page, 'parental.relock');
  await page.goto('/#/live');

  await page.getByRole('button', { name: `Watch ${first.name}, locked` }).click();
  const dialog = page.getByRole('dialog', { name: 'Parental PIN' });
  await expect(dialog).toBeVisible();
  await dialog.getByRole('button', { name: 'Back' }).click();
  await expect(dialog).toBeHidden();
  await expect(page.getByTestId('notices')).toContainText('locked');
  expect((await invoke<{ channelId: number | null }>(page, 'player.state')).channelId)
    .not.toBe(first.id);
});

test('with a parental PIN set, an age limit cannot be changed without it', async ({ page }) => {
  await page.goto('/#/');
  await expect(page.getByRole('region', { name: 'Featured' })).toBeVisible();
  await invoke(page, 'profiles.setParental', { hideAdult: true, lockSettings: false, masterPin: '2468' });
  await invoke(page, 'parental.relock');
  const section = await parentalSection(page);

  await section.getByLabel('Age limit for Me').selectOption({ label: 'Up to 12' });
  const dialog = page.getByRole('dialog', { name: 'Parental PIN' });
  await expect(dialog).toBeVisible();
  await expect(dialog).toContainText('Enter the parental PIN');
  await typePin(page, '2468');
  await expect(dialog).toBeHidden();
  // Saved on the retry, once the PIN opened it.
  await expect(section.getByLabel('Age limit for Me')).toHaveValue('12');
  await expect.poll(async () =>
    (await invoke<{ maxAge: number | null }[]>(page, 'profiles.list'))[0]!.maxAge).toBe(12);
});

test('Settings can be locked behind the PIN', async ({ page }) => {
  await page.goto('/#/');
  await expect(page.getByRole('region', { name: 'Featured' })).toBeVisible();
  await invoke(page, 'profiles.setParental', { hideAdult: true, lockSettings: true, masterPin: '2468' });
  await invoke(page, 'parental.relock');

  await page.goto('/#/settings');
  await expect(page.getByRole('heading', { name: 'Settings are locked' })).toBeVisible();
  await expect(page.getByRole('region', { name: 'Parental controls' })).toBeHidden();
  await page.getByRole('button', { name: 'Enter the PIN' }).click();
  await typePin(page, '2468');
  await expect(page.getByRole('region', { name: 'Parental controls' })).toBeVisible();
});

test('a profile that blocks unrated titles is told why its film library is empty', async ({ page }) => {
  // `?unenriched`: a panel's library before metadata is fetched, with no ratings at all.
  await page.goto('/?unenriched#/');
  await expect(page.getByRole('region', { name: 'Featured' })).toBeVisible();
  await invoke(page, 'profiles.setLimits', {
    profileId: 1, maxAge: 12, allowUnrated: false, dailyLimitMin: null,
  });
  await invoke(page, 'profiles.select', { profileId: 1 });

  await page.goto('/?unenriched#/movies');
  await expect(page.getByText('Nothing rated for this profile yet')).toBeVisible();
  await expect(page.getByText("none of this library's are rated")).toBeVisible();

  const section = await parentalSection(page, '/?unenriched');
  await expect(section.getByTestId('rated-share'))
    .toContainText('None of your films and series carry a rating yet');

  // Letting unrated titles through brings the library back.
  await invoke(page, 'profiles.setLimits', {
    profileId: 1, maxAge: 12, allowUnrated: true, dailyLimitMin: null,
  });
  await page.goto('/?unenriched#/movies');
  await expect.poll(async () =>
    Number(await page.getByTestId('browse-grid').getAttribute('data-loaded') ?? 0)).toBeGreaterThan(0);
});
