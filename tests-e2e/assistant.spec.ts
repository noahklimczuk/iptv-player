/**
 * The assistant (README §11).
 *
 * The journeys worth having are the ones about the *shape* of an answer rather than its
 * content: a question goes up immediately, the working is shown while it thinks, the
 * reply carries cards, and a card plays. The mock cannot reason — it matches keywords —
 * but it answers in exactly the shape the host does, which is what these assert.
 *
 * What a browser cannot show: whether the model is any good, and whether its tool calls
 * are sensible. That needs a key and a real library; `tests-host/scenarios` is where the
 * host side is checked.
 */
import { expect, test, type Page } from '@playwright/test';

const SHOTS = 'screenshots';

/** The assistant is gated on a key, the way the recommendation rail is. */
async function giveItAKey(page: Page) {
  await page.goto('/#/settings');
  await expect(page.getByRole('heading', { name: 'Settings' })).toBeVisible();
  await page.evaluate(async () => {
    const w = window as unknown as { __auroraInvoke?: (c: string, a: unknown) => Promise<unknown> };
    // Through the same transport the UI uses, so the mock's own state is what changes.
    await w.__auroraInvoke!('gemini.setKey', { key: 'test-key' });
  });
}

async function openAssistant(page: Page) {
  await page.goto('/#/assistant');
  await expect(page.getByRole('heading', { name: 'Assistant' })).toBeVisible();
}

test('without a key it says so instead of pretending', async ({ page }) => {
  await openAssistant(page);
  await expect(page.getByText('The assistant needs a key')).toBeVisible();
  await expect(page.getByTestId('assistant-input')).toBeHidden();
});

test('a question comes back with titles that can be played', async ({ page }) => {
  await giveItAKey(page);
  await openAssistant(page);

  await page.getByTestId('assistant-input').fill('something tense');
  await page.getByTestId('assistant-send').click();

  // The question is on screen as its own turn.
  await expect(page.getByTestId('assistant-user')).toContainText('something tense');

  // And the answer carries cards.
  const reply = page.getByTestId('assistant-assistant');
  await expect(reply).toBeVisible();
  const cards = page.locator('[data-testid^="assistant-item-"]');
  await expect(cards.first()).toBeVisible();
  await page.screenshot({ path: `${SHOTS}/assistant-reply.png` });

  // Each card says why it was offered, which is the difference between a
  // recommendation and a list.
  await expect(cards.first()).toContainText(/—/);

  // Pressing play on one opens the player on it.
  await page.locator('[data-testid^="assistant-play-"]').first().click();
  await expect(page.getByRole('button', { name: 'Pause' })).toBeVisible();
});

test('the openers work for somebody who does not know what to ask', async ({ page }) => {
  await giveItAKey(page);
  await openAssistant(page);

  const first = page.getByTestId('assistant-suggestion').first();
  const asked = (await first.textContent())!.trim();
  await first.click();

  await expect(page.getByTestId('assistant-user')).toContainText(asked);
  await expect(page.getByTestId('assistant-assistant')).toBeVisible();
  // The openers go away once there is a conversation.
  await expect(page.getByTestId('assistant-suggestion')).toHaveCount(0);
});

test('it can answer about the library itself', async ({ page }) => {
  await giveItAKey(page);
  await openAssistant(page);

  await page.getByTestId('assistant-input').fill('how many films do I have?');
  await page.getByTestId('assistant-send').click();

  // A count, not a pile of cards: "what have I got" is a question about the library.
  await expect(page.getByTestId('assistant-assistant')).toContainText(/\d+ films/);
  await expect(page.locator('[data-testid^="assistant-item-"]')).toHaveCount(0);
});

test('the conversation is remembered, and can be cleared', async ({ page }) => {
  await giveItAKey(page);
  await openAssistant(page);

  await page.getByTestId('assistant-input').fill('a comedy');
  await page.getByTestId('assistant-send').click();
  await expect(page.getByTestId('assistant-assistant')).toBeVisible();

  // Leave and come back: the thread is still there, because the host keeps it.
  await page.goto('/#/live');
  await expect(page.getByRole('heading', { name: 'Live TV' })).toBeVisible();
  await openAssistant(page);
  await expect(page.getByTestId('assistant-user')).toContainText('a comedy');

  await page.getByTestId('assistant-clear').click();
  await expect(page.getByTestId('assistant-user')).toHaveCount(0);
  await expect(page.getByTestId('assistant-suggestion').first()).toBeVisible();
});

test('a refusal is reported rather than left as a silent nothing', async ({ page }) => {
  await giveItAKey(page);
  await openAssistant(page);

  await page.evaluate(() => {
    const w = window as unknown as { __auroraFailNext?: (n: string, m: string) => void };
    w.__auroraFailNext!('assistant.send', 'The model is over its quota');
  });

  await page.getByTestId('assistant-input').fill('anything');
  await page.getByTestId('assistant-send').click();

  // The question stays — it is still what they asked — and the failure is said out loud
  // rather than leaving a turn that never answers.
  await expect(page.getByTestId('assistant-user')).toContainText('anything');
  await expect(page.getByText('The model is over its quota')).toBeVisible();
  await expect(page.getByTestId('assistant-assistant')).toContainText(/could not answer/);
});
