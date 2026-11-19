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

test('a kids profile is kept out of Settings, and the host refuses what is behind them', async ({ page }) => {
  await page.goto('/#/');
  await expect(page.getByRole('region', { name: 'Featured' })).toBeVisible();
  const sam = await invoke<number>(page, 'profiles.create', {
    name: 'Sam', avatar: null, isKids: true, maxAge: 12, dailyLimitMin: null,
  });
  // The switch for everybody stays off: a kids profile is locked out without it.
  await invoke(page, 'profiles.setParental', { hideAdult: true, lockSettings: false, masterPin: '2468' });
  await invoke(page, 'profiles.select', { profileId: sam });

  await page.goto('/#/settings');
  await expect(page.getByRole('heading', { name: 'Settings are locked' })).toBeVisible();
  await expect(page.getByRole('region', { name: 'Parental controls' })).toBeHidden();

  // Reaching a command behind Settings some other way asks for the PIN too, and
  // cancelling it leaves the stream password unread.
  // Settled into a string at once: it rejects while the test is still pressing Escape,
  // and a rejection nobody is listening for yet fails the test on its own.
  const credentials = invoke(page, 'providers.credentials', { providerId: 1 })
    .then(() => 'answered', (e: Error) => e.message);
  const dialog = page.getByRole('dialog', { name: 'Parental PIN' });
  await expect(dialog).toBeVisible();
  await page.keyboard.press('Escape');
  expect(await credentials).toContain('Enter the parental PIN to change this');

  await page.getByRole('button', { name: 'Enter the PIN' }).click();
  await typePin(page, '2468');
  await expect(page.getByRole('region', { name: 'Parental controls' })).toBeVisible();

  // A grown-up's profile is not locked out.
  await page.goto('/#/');
  await invoke(page, 'profiles.select', { profileId: 1 });
  await page.goto('/#/settings');
  await expect(page.getByRole('region', { name: 'Parental controls' })).toBeVisible();
});

test('adult categories show only with the parental PIN, and removing it hides them again', async ({ page }) => {
  const section = await parentalSection(page);
  const adult = section.getByRole('switch', { name: 'Hide adult categories' });

  // No PIN: hidden, and nothing here can show them.
  await expect(adult).toHaveAttribute('aria-checked', 'true');
  await expect(adult).toBeDisabled();
  await expect(section).toContainText('Set a parental PIN first to show them');
  const refused = await invoke(page, 'profiles.setParental', { hideAdult: false, lockSettings: false })
    .then(() => 'saved', (e: Error) => e.message);
  expect(refused).toContain('Set a parental PIN before showing adult categories');

  // With a PIN, showing them is a choice.
  await section.getByRole('button', { name: 'Set a PIN' }).click();
  await section.getByLabel('New parental PIN', { exact: true }).fill('2468');
  await section.getByLabel('New parental PIN again').fill('2468');
  await section.getByRole('button', { name: 'Save PIN' }).click();
  await expect(adult).toBeEnabled();
  await adult.click();
  await expect(adult).toHaveAttribute('aria-checked', 'false');

  // Removing the PIN hides them again.
  await section.getByRole('button', { name: 'Remove PIN' }).click();
  await expect(section.getByRole('button', { name: 'Set a PIN' })).toBeVisible();
  await expect(adult).toHaveAttribute('aria-checked', 'true');
  await expect(adult).toBeDisabled();
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

test('a daily limit warns, stops playback at the limit, and the PIN gives more time', async ({ page }) => {
  await page.goto('/#/');
  await expect(page.getByRole('region', { name: 'Featured' })).toBeVisible();
  // Choosing a profile closes what the PIN opened, so it comes first; setting the PIN
  // then opens the parental settings for a while, as entering it would.
  await invoke(page, 'profiles.select', { profileId: 1 });
  await invoke(page, 'profiles.setParental', { hideAdult: true, lockSettings: false, masterPin: '2468' });

  const section = await parentalSection(page);
  await section.getByLabel('Daily limit for Me').selectOption({ label: '1 hour a day' });
  await expect.poll(async () =>
    (await invoke<{ dailyLimitMin: number | null }[]>(page, 'profiles.list'))[0]!.dailyLimitMin).toBe(60);
  await invoke(page, 'parental.relock');
  await page.goto('/#/');

  const watched = (secs: number) => page.evaluate(
    (s) => (window as unknown as { __auroraWatched: (s: number) => void }).__auroraWatched(s),
    secs,
  );
  await watched(58 * 60);
  await expect(page.getByText('2 minutes of viewing left today')).toBeVisible();

  const films = await invoke<{ id: number }[]>(page, 'library.movies', { sort: 'title', limit: 1, offset: 0 });
  await invoke(page, 'player.play', { kind: 'movie', id: films[0]!.id });
  await watched(60 * 60);
  await expect(page.getByText("Today's viewing time is used up").first()).toBeVisible();
  await expect.poll(async () =>
    (await invoke<{ status: string }>(page, 'player.state')).status).toBe('idle');

  // Playing again asks for the PIN, and the PIN gives more time.
  const again = invoke(page, 'player.play', { kind: 'movie', id: films[0]!.id });
  const dialog = page.getByRole('dialog', { name: 'Parental PIN' });
  await expect(dialog).toContainText("Today's viewing time is used up");
  await typePin(page, '2468');
  await again;
  await expect.poll(async () =>
    (await invoke<{ status: string }>(page, 'player.state')).status).toBe('playing');
});
