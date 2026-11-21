/**
 * Settings → Account and sync → Backup (`aurora-app/src/backup.rs`).
 *
 * The file format and the restore itself are tested in Rust (`aurora-sync/tests/backup.rs`):
 * a fresh install, a provider added again by hand, sync's state untouched, no password in
 * the file. This is what the card makes of them: saving says where the file went,
 * restoring says what a file holds before writing anything, and the wrong file, or a
 * locked household, is refused in words.
 */
import { expect, test, type Page } from '@playwright/test';

const SHOTS = 'screenshots';

const BACKUP = JSON.stringify({
  format: 'aurora-backup',
  version: 1,
  createdAt: 1_795_270_000,
  appVersion: '2.0.0',
  records: [
    { kind: 'provider', ident: { id: 'p1' }, fields: {} },
    { kind: 'profile', ident: { id: 'a' }, fields: {} },
    { kind: 'profile', ident: { id: 'b' }, fields: {} },
    { kind: 'favorite', ident: { id: 1 }, fields: {} },
    { kind: 'favorite', ident: { id: 2 }, fields: {} },
    { kind: 'setting', ident: { key: 'filter.english_only' }, fields: {} },
  ],
});

async function backupCard(page: Page) {
  await page.goto('/#/settings');
  await expect(page.getByRole('heading', { name: 'Settings' })).toBeVisible();
  const card = page.getByRole('group', { name: 'Backup', exact: true });
  await card.scrollIntoViewIfNeeded();
  await expect(card).toBeVisible();
  return card;
}

async function pick(page: Page, name: string, text: string) {
  await page.getByLabel('Backup file to restore').setInputFiles({
    name, mimeType: 'application/json', buffer: Buffer.from(text),
  });
}

test('a backup is saved to the Backups folder and says what went in it', async ({ page }) => {
  const card = await backupCard(page);
  await expect(card).toContainText('Passwords are never saved in it');
  await card.getByRole('button', { name: 'Save a backup' }).click();
  const done = card.getByRole('status');
  await expect(done).toContainText('Documents\\Aurora TV\\Backups\\aurora-tv-backup-');
  await expect(done).toContainText(/1 provider, \d+ profiles?, 3 favourites and 12 settings/);
  await expect(done.getByRole('button', { name: 'Show in folder' })).toBeVisible();
  await page.screenshot({ path: `${SHOTS}/backup-saved.png` });
});

test('a backup file is shown before it is restored, and restoring asks for the passwords', async ({ page }) => {
  const card = await backupCard(page);
  await pick(page, 'aurora-tv-backup-2026-11-21-1406.json', BACKUP);

  const preview = card.getByRole('group', { name: 'Backup to restore' });
  await expect(preview).toContainText('aurora-tv-backup-2026-11-21-1406.json');
  await expect(preview).toContainText('1 provider, 2 profiles, 2 favourites and 1 setting');
  await expect(preview).toContainText('by Aurora 2.0.0');
  await expect(preview).toContainText('Nothing is deleted');
  await page.screenshot({ path: `${SHOTS}/backup-restore-preview.png` });

  // Cancel writes nothing and clears the preview.
  await preview.getByRole('button', { name: 'Cancel' }).click();
  await expect(preview).toHaveCount(0);

  await pick(page, 'aurora-tv-backup-2026-11-21-1406.json', BACKUP);
  await card.getByRole('group', { name: 'Backup to restore' })
    .getByRole('button', { name: 'Restore', exact: true }).click();
  const done = card.getByRole('status');
  await expect(done).toContainText('Restored 6 items.');
  await expect(done).toContainText('enter each provider’s again in Settings → Providers');
});

test('anything but an Aurora backup is refused before anything is written', async ({ page }) => {
  const card = await backupCard(page);
  await pick(page, 'holiday.json', '{"photos": []}');
  await expect(card.getByRole('alert')).toContainText('this is not an Aurora backup file');
  await expect(card.getByRole('group', { name: 'Backup to restore' })).toHaveCount(0);

  await pick(page, 'future.json', JSON.stringify({ format: 'aurora-backup', version: 9, records: [] }));
  await expect(card.getByRole('alert')).toContainText('made by a newer Aurora');
});

test('with a parental PIN, restoring asks for it first', async ({ page }) => {
  await page.goto('/#/settings');
  const parental = page.getByRole('region', { name: 'Parental controls' });
  await parental.scrollIntoViewIfNeeded();
  await parental.getByRole('button', { name: 'Set a PIN' }).click();
  await parental.getByLabel('New parental PIN', { exact: true }).fill('2468');
  await parental.getByLabel('New parental PIN again').fill('2468');
  await parental.getByRole('button', { name: 'Save PIN' }).click();
  await expect(parental.getByRole('button', { name: 'Change PIN' })).toBeVisible();
  await page.evaluate(() => (window as unknown as {
    __auroraInvoke: (c: string, a: unknown) => Promise<unknown>;
  }).__auroraInvoke('parental.relock', undefined));

  const card = page.getByRole('group', { name: 'Backup', exact: true });
  await card.scrollIntoViewIfNeeded();
  await pick(page, 'aurora-tv-backup.json', BACKUP);
  await card.getByRole('group', { name: 'Backup to restore' })
    .getByRole('button', { name: 'Restore', exact: true }).click();

  const dialog = page.getByRole('dialog', { name: 'Parental PIN' });
  await expect(dialog).toBeVisible();
  for (const d of '2468') await dialog.getByRole('button', { name: d, exact: true }).click();
  await expect(card.getByRole('status')).toContainText('Restored 6 items.');
});
