/**
 * What the DVR already knows about the guide entries on screen, and the actions that
 * change it (README §7.7).
 *
 * The guide needs two answers per programme — "is this being recorded" and "is there a
 * reminder" — for every cell it paints, so this fetches the whole scheduled set once and
 * answers from a lookup rather than a command per cell.
 */
import { useCallback, useMemo, useState } from 'react';
import type { Channel, Programme, Recording } from '@shared/ipc';
import { useCommand } from '@/hooks/useCommand';
import { invoke, onDvrTick } from '@/ipc';
import { useEffect } from 'react';

/** How the host identifies one airing: channel, airtime, title. */
const keyOf = (channelId: number, title: string, start: number) =>
  `${channelId}:${start}:${title}`;

export interface DvrMarks {
  /** The recording for this airing, or null. */
  recordingFor: (ch: Channel, p: Programme) => Recording | null;
  /** The reminder id for this airing, or null. */
  reminderFor: (ch: Channel, p: Programme) => number | null;
  /** Whether a series rule already covers this title. */
  ruleFor: (p: Programme) => number | null;
  toggleRecord: (ch: Channel, p: Programme) => Promise<void>;
  toggleReminder: (ch: Channel, p: Programme) => Promise<void>;
  toggleSeries: (ch: Channel, p: Programme, anyChannel: boolean) => Promise<void>;
  /** Keys of every airing with a recording, for painting the grid. */
  recordingKeys: Set<string>;
  refresh: () => void;
  busy: boolean;
  error: string | null;
}

export function useDvrMarks(): DvrMarks {
  const [nonce, setNonce] = useState(0);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const refresh = useCallback(() => setNonce((n) => n + 1), []);

  // Everything not yet finished: scheduled and in flight both count as "recording this".
  const { data: pending } = useCommand('dvr.list', { state: 'scheduled' }, [nonce]);
  const { data: live } = useCommand('dvr.list', { state: 'recording' }, [nonce]);
  const { data: reminders } = useCommand('dvr.reminders', undefined, [nonce]);
  const { data: rules } = useCommand('dvr.rules', undefined, [nonce]);

  useEffect(() => onDvrTick(() => refresh()), [refresh]);

  const byAiring = useMemo(() => {
    const m = new Map<string, Recording>();
    for (const r of [...(pending ?? []), ...(live ?? [])]) {
      m.set(keyOf(r.channelId, r.title, r.airStart), r);
    }
    return m;
  }, [pending, live]);

  const reminderKeys = useMemo(() => {
    const m = new Map<string, number>();
    for (const r of reminders ?? []) m.set(keyOf(r.channelId, r.title, r.start), r.id);
    return m;
  }, [reminders]);

  const ruleTitles = useMemo(() => {
    const m = new Map<string, number>();
    // Titles are compared case-insensitively, as the host's match_key does.
    for (const r of rules ?? []) if (r.enabled) m.set(r.title.toLowerCase(), r.id);
    return m;
  }, [rules]);

  /** Run an action, surface its message rather than throwing into the render tree. */
  const run = useCallback(async (fn: () => Promise<unknown>) => {
    setBusy(true);
    setError(null);
    try {
      await fn();
      refresh();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }, [refresh]);

  return {
    recordingKeys: useMemo(() => new Set(byAiring.keys()), [byAiring]),
    recordingFor: (ch, p) => byAiring.get(keyOf(ch.id, p.title, p.start)) ?? null,
    reminderFor: (ch, p) => reminderKeys.get(keyOf(ch.id, p.title, p.start)) ?? null,
    ruleFor: (p) => ruleTitles.get(p.title.toLowerCase()) ?? null,

    toggleRecord: (ch, p) => run(async () => {
      const existing = byAiring.get(keyOf(ch.id, p.title, p.start));
      if (existing) {
        await invoke('dvr.cancel', { id: existing.id });
        return;
      }
      await invoke('dvr.schedule', {
        channelId: ch.id,
        title: p.title,
        subTitle: p.subTitle,
        season: p.season,
        episode: p.episode,
        airStart: p.start,
        airStop: p.stop,
      });
    }),

    toggleReminder: (ch, p) => run(async () => {
      const existing = reminderKeys.get(keyOf(ch.id, p.title, p.start));
      if (existing) {
        await invoke('dvr.removeReminder', { id: existing });
        return;
      }
      await invoke('dvr.addReminder', { channelId: ch.id, title: p.title, start: p.start });
    }),

    toggleSeries: (ch, p, anyChannel) => run(async () => {
      const existing = ruleTitles.get(p.title.toLowerCase());
      if (existing) {
        await invoke('dvr.deleteRule', { id: existing });
        return;
      }
      await invoke('dvr.createRule', {
        title: p.title,
        // "This channel only" is the safer default: the same title on another
        // provider's channel is usually a different broadcast, not the same show.
        channelId: anyChannel ? null : ch.id,
        newOnly: true,
      });
    }),

    refresh,
    busy,
    error,
  };
}
