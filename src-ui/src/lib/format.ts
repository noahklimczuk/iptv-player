/** Formatting helpers. All times render in the viewer's local zone (README §4.4). */

export const pad2 = (n: number) => String(n).padStart(2, '0');

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

/** "2h 14m left" for the continue-watching rail. */
export function remaining(positionSecs: number, durationSecs: number): string {
  const left = Math.max(0, durationSecs - positionSecs);
  const mins = Math.round(left / 60);
  return `${runtime(mins)} left`;
}

export const progressPct = (pos: number, dur: number) =>
  dur > 0 ? Math.min(100, Math.max(0, (pos / dur) * 100)) : 0;
