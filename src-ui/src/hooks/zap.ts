/**
 * The zapper's arithmetic, which `vitest.config.ts` names as one of the three gaps it
 * was written to point at.
 *
 * Extracted so it can be tested without a React renderer. The wrap-around is the part
 * worth pinning: Ch+ from the last channel is the first one, and Ch− from the first is
 * the last, which is what a set-top box does and what the modulo is for (R-161).
 */

/**
 * Which channel Ch+ / Ch− should land on, or null when there are none.
 *
 * `current` is the index of the channel playing now, or -1 for nothing yet.
 *
 * The cold-start case was wrong. `(i + delta + n) % n` with `i = -1` gives `n - 2` for
 * Ch−, so the very first press of Ch− from a standing start skipped the last channel and
 * landed on the second-to-last. Ch+ from the same state was right by accident, because
 * `-1 + 1` is 0.
 */
export function stepIndex(count: number, current: number, delta: 1 | -1): number | null {
  if (count <= 0) return null;
  // From nothing, Ch+ is the first channel and Ch− is the last: both are a step from
  // *outside* the list, not from an imaginary channel before the first one.
  if (current < 0) return delta === 1 ? 0 : count - 1;
  return (current + delta + count) % count;
}
