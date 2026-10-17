/**
 * Ch+ and Ch− at the edges of the list, which nothing covered.
 *
 * `vitest.config.ts` names "the zapper's arithmetic" as one of the three things it was
 * written for; the one that got closed was `subscribe`, which is none of them (R-161).
 */
import { describe, expect, it } from 'vitest';
import { stepIndex } from './zap';

describe('stepIndex', () => {
  it('wraps at both ends, the way a set-top box does', () => {
    expect(stepIndex(5, 0, 1)).toBe(1);
    expect(stepIndex(5, 4, 1)).toBe(0);
    expect(stepIndex(5, 4, -1)).toBe(3);
    expect(stepIndex(5, 0, -1)).toBe(4);
  });

  it('starts at the right end when nothing is playing yet', () => {
    // Ch+ from nothing is the first channel, which `(i + delta + n) % n` got right by
    // accident. Ch− from nothing used to be `n - 2` — the second-to-last channel, with
    // the last one skipped entirely on the first press.
    expect(stepIndex(42, -1, 1)).toBe(0);
    expect(stepIndex(42, -1, -1)).toBe(41);
  });

  it('has nowhere to go in an empty list', () => {
    expect(stepIndex(0, -1, 1)).toBeNull();
    expect(stepIndex(0, 3, -1)).toBeNull();
  });

  it('is a single channel in a list of one', () => {
    expect(stepIndex(1, 0, 1)).toBe(0);
    expect(stepIndex(1, 0, -1)).toBe(0);
    expect(stepIndex(1, -1, -1)).toBe(0);
  });
});
