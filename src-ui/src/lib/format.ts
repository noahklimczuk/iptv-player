/** Formatting helpers. All times render in the viewer's local zone (README §4.4). */

export const pad2 = (n: number) => String(n).padStart(2, '0');

/**
 * A time of day, in the notation asked for.
 *
 * The preference existed in three halves and was joined up by none of them: `use24h` was
 * declared and initialised in the store, honoured here *if anyone passed it*, and
 * nothing read it -- none of the twenty-odd call sites passed a second argument, and
 * there was no action to set it beside `setTheme` and `setDensity`. So the AM/PM branch
 * below was unreachable and the setting was a field nobody could change (R-136).
 *
 * This stays a pure function -- it is in the module every screen imports for
 * formatting, and importing the store here pulled `window` into a node-environment unit
 * test. `useClockTime` in `hooks/` is the bound form, and is what a component uses.
 */
export function clockTime(unix: number, use24h = true): string {
  const d = new Date(unix * 1000);
  if (use24h) return `${pad2(d.getHours())}:${pad2(d.getMinutes())}`;
  const h = d.getHours() % 12 || 12;
  return `${h}:${pad2(d.getMinutes())} ${d.getHours() < 12 ? 'AM' : 'PM'}`;
}

export function runtime(mins: number | null | undefined): string {
  if (!mins || mins <= 0) return '';
  const h = Math.floor(mins / 60);
  const m = mins % 60;
  return h ? `${h}h ${m}m` : `${m}m`;
}

export function duration(secs: number): string {
  if (!Number.isFinite(secs) || secs < 0) secs = 0;
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  const s = Math.floor(secs % 60);
  return h ? `${h}:${pad2(m)}:${pad2(s)}` : `${m}:${pad2(s)}`;
}

export function dayLabel(unix: number): string {
  const d = new Date(unix * 1000);
  const today = new Date();
  const startOf = (x: Date) => new Date(x.getFullYear(), x.getMonth(), x.getDate()).getTime();
  const diff = Math.round((startOf(d) - startOf(today)) / 86400000);
  if (diff === 0) return 'Today';
  if (diff === 1) return 'Tomorrow';
  if (diff === -1) return 'Yesterday';
  return d.toLocaleDateString(undefined, { weekday: 'short', day: 'numeric', month: 'short' });
}

/**
 * "2h 14m left" for the continue-watching rail.
 *
 * The floor matters. `runtime` returns the empty string for zero -- deliberately, so a
 * title with no known runtime renders nothing -- and this concatenated it anyway, so a
 * card with 20 seconds to go read " left", with nothing in front of it. `runtime` cannot
 * tell "zero minutes" from "unknown" and this has no business asking it to (R-95).
 */
export function remaining(positionSecs: number, durationSecs: number): string {
  const left = Math.max(0, durationSecs - positionSecs);
  const mins = Math.round(left / 60);
  if (mins <= 0) return 'Less than a minute left';
  return `${runtime(mins)} left`;
}

export const progressPct = (pos: number, dur: number) =>
  dur > 0 ? Math.min(100, Math.max(0, (pos / dur) * 100)) : 0;

/**
 * Disk sizes, as a recordings library reports them.
 *
 * The carry has to survive the rounding. `while (v >= 1024)` ran first and the rounding
 * after it, so a value just under a boundary carried too late: 1,048,575 bytes printed
 * **1024 KB** and 1,023.6 printed **1024 B**. The window is the last 0.05% of each unit,
 * and the storage line it appears on is read while deciding whether there is room --
 * which is exactly when a number one unit too small is unhelpful (R-96).
 */
export function bytes(n: number): string {
  if (!Number.isFinite(n) || n <= 0) return '0 MB';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  let i = 0;
  let v = n;
  // One decimal below 10 so "1.4 GB" does not round to "1 GB". Kept as the string it
  // will be printed as, so the carry below is tested against what the viewer sees
  // rather than against the raw value -- which is the whole defect.
  const shown = (value: number, unit: number) =>
    (value < 10 && unit > 1 ? value.toFixed(1) : String(Math.round(value)));
  while (Number(shown(v, i)) >= 1024 && i < units.length - 1) {
    v /= 1024;
    i += 1;
  }
  return `${shown(v, i)} ${units[i]}`;
}

/**
 * "in 3h 20m" / "in 4 min" — how long until a scheduled recording starts.
 *
 * Both parts are floored. `h` was floored and `m` **rounded**, so the two disagreed
 * whenever the leftover was in the last half-minute of an hour: measured over every
 * second in the hour-to-day window, 690 of them rendered "Xh 60m", with "in 23h 60m" as
 * the worst case. That is 11.5 minutes in every 24 hours of lead time, on the scheduled
 * recordings list. `runtime()` has the same job three functions above and floors both
 * (R-94).
 *
 * Its day-out branch renders a clock time, so it carries the notation too -- it was the
 * one `clockTime` caller the preference could not reach, because it is a function rather
 * than a component and so took the parameter default (R-136). `useUntilLabel` is the
 * bound form.
 */
export function untilLabel(
  unix: number,
  now = Math.floor(Date.now() / 1000),
  use24h = true,
): string {
  const secs = unix - now;
  if (secs <= 0) return 'now';
  if (secs < 3600) return `in ${Math.max(1, Math.round(secs / 60))} min`;
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  if (secs < 86400) return m ? `in ${h}h ${m}m` : `in ${h}h`;
  return `${dayLabel(unix)} at ${clockTime(unix, use24h)}`;
}
