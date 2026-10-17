/**
 * The formatters, which `vitest.config.ts` was written to point at and nothing covered.
 *
 * Its comment names three gaps — `shared/ipc.ts`, "the formatters", and "the zapper's
 * arithmetic" — and the one that got closed was `subscribe`, which is none of them. These
 * are pure functions of their inputs in a file with no dependencies: the easiest thing in
 * the repository to test, and three live defects deep (R-161).
 */
import { describe, expect, it } from 'vitest';
import { bytes, duration, progressPct, remaining, runtime, untilLabel } from './format';

describe('untilLabel', () => {
  /**
   * "in 1h 60m", for 690 seconds of every day's worth of lead time.
   *
   * `h` was floored and `m` rounded, so the two disagreed whenever the leftover was in
   * the last half-minute of an hour. The worst case is "in 23h 60m", which should not
   * even be in this branch (R-94).
   */
  it('never renders sixty minutes', () => {
    const bad: string[] = [];
    // Every second of the hour-to-day window, which is what the finding measured.
    for (let secs = 3600; secs < 86400; secs += 1) {
      const label = untilLabel(secs, 0);
      if (/\b60m\b/.test(label)) bad.push(`${secs} -> ${label}`);
    }
    expect(bad.slice(0, 5), `${bad.length} offsets render 60m`).toEqual([]);
  });

  it('floors both halves, so it never rounds up into the next hour', () => {
    // 1h 59m 50s is 1h 59m, not 2h: the honest answer is the one that has not happened
    // yet, and rounding here used to produce "in 2h" and "in 1h 60m" alternately.
    expect(untilLabel(7190, 0)).toBe('in 1h 59m');
    expect(untilLabel(7170, 0)).toBe('in 1h 59m');
    expect(untilLabel(3600, 0)).toBe('in 1h');
    expect(untilLabel(7260, 0)).toBe('in 2h 1m');
  });

  it('says the obvious things', () => {
    expect(untilLabel(0, 0)).toBe('now');
    expect(untilLabel(-30, 0)).toBe('now');
    // Under a minute still reads as a minute rather than as "in 0 min".
    expect(untilLabel(20, 0)).toBe('in 1 min');
    expect(untilLabel(240, 0)).toBe('in 4 min');
  });

  it('hands anything a day out to the day label', () => {
    // Which is the branch "in 23h 60m" belonged in.
    expect(untilLabel(86400 * 2, 0)).toMatch(/ at \d/);
  });
});

describe('remaining', () => {
  /** `runtime(0)` is the empty string by design, and this concatenated it (R-95). */
  it('never renders "left" with nothing in front of it', () => {
    for (const left of [1, 10, 20, 29]) {
      const label = remaining(600 - left, 600);
      expect(label.trim(), `${left}s left rendered "${label}"`).not.toBe('left');
      expect(label).toMatch(/\S/);
    }
    expect(remaining(580, 600)).toBe('Less than a minute left');
  });

  it('reads as a time otherwise', () => {
    expect(remaining(0, 8040)).toBe('2h 14m left');
    expect(remaining(3600, 8040)).toBe('1h 14m left');
    expect(remaining(7800, 8040)).toBe('4m left');
    // A position past the end is not negative time.
    expect(remaining(9000, 8040)).toBe('Less than a minute left');
  });
});

describe('bytes', () => {
  /** The carry ran before the rounding, so a value just under a boundary carried too
   *  late: 1,048,575 printed "1024 KB" and 1,023.6 printed "1024 B" (R-96). */
  it('never prints 1024 of a unit', () => {
    const bad: string[] = [];
    for (const n of [1023.6, 1048575, 1048575.9, 1073741823, 1099511627775, 1024 * 1024 - 1]) {
      const out = bytes(n);
      if (/^1024\b/.test(out)) bad.push(`${n} -> ${out}`);
    }
    expect(bad).toEqual([]);
    expect(bytes(1048575)).toBe('1.0 MB');
    expect(bytes(1023.6)).toBe('1 KB');
  });

  it('reads the way a storage line should', () => {
    expect(bytes(0)).toBe('0 MB');
    expect(bytes(-1)).toBe('0 MB');
    expect(bytes(512)).toBe('512 B');
    expect(bytes(1024)).toBe('1 KB');
    expect(bytes(1024 * 1024)).toBe('1.0 MB');
    expect(bytes(1.4 * 1024 ** 3)).toBe('1.4 GB');
    // One decimal only below 10, which is the existing rule and a sensible one.
    expect(bytes(30.6 * 1024 ** 2)).toBe('31 MB');
  });
});

describe('runtime and duration', () => {
  it('renders a runtime the way the cards do', () => {
    expect(runtime(null)).toBe('');
    expect(runtime(0)).toBe('');
    expect(runtime(7)).toBe('7m');
    expect(runtime(60)).toBe('1h 0m');
    expect(runtime(134)).toBe('2h 14m');
  });

  it('renders a playhead the way the OSD does', () => {
    expect(duration(0)).toBe('0:00');
    expect(duration(59)).toBe('0:59');
    expect(duration(61)).toBe('1:01');
    expect(duration(3600)).toBe('1:00:00');
    expect(duration(3661)).toBe('1:01:01');
    // Nothing on screen may read "NaN:NaN".
    expect(duration(Number.NaN)).toBe('0:00');
    expect(duration(-5)).toBe('0:00');
    expect(duration(Number.POSITIVE_INFINITY)).toBe('0:00');
  });

  it('keeps a progress bar inside its track', () => {
    expect(progressPct(0, 100)).toBe(0);
    expect(progressPct(50, 100)).toBe(50);
    expect(progressPct(150, 100)).toBe(100);
    expect(progressPct(-10, 100)).toBe(0);
    // A duration of zero is "unknown", not a division.
    expect(progressPct(10, 0)).toBe(0);
  });
});
