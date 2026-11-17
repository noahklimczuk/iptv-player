/**
 * Everything on the picture that is not the dock: the top bar (C17), the live cards,
 * the stats panel (C18), the loading veil (C24), the error panel (C25) and the sleep
 * timer's warning.
 */
import type { PlayerState } from '@shared/ipc';
import { artHue } from '@/components/Primitives';
import { Icon } from '@/components/Icon';
import type { NowNext } from '@/features/live/useNowNext';
import { invoke } from '@/ipc';
import { notify, report } from '@/lib/errors';
import { duration } from '@/lib/format';
import { useClockTime } from '@/hooks/useClockTime';
import * as control from './playerControl';
import { CHROME_ATTR } from './useChrome';
import { behindLive, currentChoices } from './model';
import { SLEEP_EXTEND_MINUTES } from './model';
import { sleepVerb, type SleepTimer } from './WatchMenu';

const chrome = { [CHROME_ATTR]: '' };

/* ── Top bar (C17, C18) ─────────────────────────────────────────────────── */

export function TopBar({
  player, onBack, statsOpen, onStats,
}: {
  player: PlayerState;
  onBack: () => void;
  statsOpen: boolean;
  onStats: () => void;
}) {
  const behind = behindLive(player);
  const name = player.title ?? 'Nothing playing';
  const { subtitles, audio } = currentChoices(player);
  const s = player.stats;
  const chips = [
    `CC ${subtitles}`,
    audio,
    s?.resolution ? [s.resolution, s.fps ? `${Math.round(s.fps)}fps` : null].filter(Boolean).join(' · ') : null,
  ].filter((c): c is string => !!c);

  return (
    <div {...chrome} className="pl-top">
      <button type="button" className="pl-btn pl-glass pl-btn-lg" aria-label="Back" title="Back" onClick={onBack}>
        <Icon name="chevronLeft" size={24} />
      </button>
      <div className="pl-title">
        <span aria-hidden className="pl-title-tile" style={{ background: artHue(name) }}>
          {name.slice(0, 1).toUpperCase()}
        </span>
        <div className="pl-title-text">
          <div className="pl-title-name">{name}</div>
          {player.subtitle && <div className="pl-title-sub">{player.subtitle}</div>}
        </div>
        {player.isLive && (
          behind > 0
            ? <span className="pl-badge pl-badge-behind">↺ {duration(behind)} behind live</span>
            : <span className="pl-badge pl-badge-live">● Live</span>
        )}
      </div>
      <div className="pl-top-right">
        {chips.map((c) => <span key={c} className="pl-tag">{c}</span>)}
        <button
          type="button"
          className="pl-btn pl-glass"
          aria-label="Playback stats"
          title="Playback stats (I)"
          aria-pressed={statsOpen}
          data-active={statsOpen}
          onClick={onStats}
        >
          <Icon name="info" size={16} />
        </button>
        {/* Live only: a film is already a file on the provider's disk. */}
        {player.isLive && player.channelId != null && (
          <button
            type="button"
            className="pl-btn pl-glass"
            aria-label="Record what's on"
            title="Record what's on"
            onClick={() => void recordNow(player)}
          >
            <Icon name="record" size={16} />
          </button>
        )}
      </div>
    </div>
  );
}

/** With no guide, a recording runs this long. */
const UNGUIDED_RECORDING_SECS = 60 * 60;
/** Joined this far into a programme, the notice says only the rest is recorded. */
const PART_GONE_SECS = 60;

/** Record from now to the end of what is on, or for an hour if the guide is silent. */
async function recordNow(player: PlayerState): Promise<void> {
  const channelId = player.channelId;
  if (channelId == null) return;
  const now = Math.floor(Date.now() / 1000);
  try {
    const guide = await invoke('epg.nowNext', { channelId }).catch(() => null);
    const on = guide?.now ?? null;
    const airStop = on?.stop ?? now + UNGUIDED_RECORDING_SECS;
    const id = await invoke('dvr.schedule', {
      channelId,
      title: on?.title ?? player.title ?? 'Recording',
      subTitle: on?.subTitle ?? null,
      airStart: now,
      airStop,
    });
    if (id == null) {
      notify('That is already being recorded');
      return;
    }
    const minutes = Math.max(1, Math.round((airStop - now) / 60));
    notify(
      on
        ? now - on.start >= PART_GONE_SECS
          ? `Recording the last ${minutes} minutes of ${on.title}`
          : `Recording ${on.title}`
        : 'Recording the next hour — this channel has no guide data',
    );
  } catch (e) {
    report('Could not start recording')(e);
  }
}

/* ── Live cards ─────────────────────────────────────────────────────────── */

/** On now at the bottom left; next at the bottom right, with a Record button. */
export function LiveCards({
  channelId, title, guide,
}: { channelId: number; title: string | null; guide: NowNext | null }) {
  const clock = useClockTime();
  const on = guide?.now ?? null;
  const next = guide?.next ?? null;

  const recordNext = () => {
    if (!next) return;
    invoke('dvr.schedule', {
      channelId,
      title: next.title,
      subTitle: next.subTitle ?? null,
      airStart: next.start,
      airStop: next.stop,
    })
      .then((id) => notify(id == null
        ? 'That is already being recorded'
        : `Recording ${next.title} at ${clock(next.start)}`))
      .catch(report('Could not schedule the recording'));
  };

  return (
    <>
      {on && (
        <div {...chrome} className="pl-card pl-card-now">
          <div className="pl-card-kicker">On now · {clock(on.start)}–{clock(on.stop)}</div>
          <div className="dl-display pl-card-title">{on.title}</div>
          {(on.description ?? on.subTitle) && <div className="pl-card-text">{on.description ?? on.subTitle}</div>}
        </div>
      )}
      {next && (
        <div {...chrome} className="pl-card pl-card-next" role="group" aria-label={`After ${title ?? 'this'}`}>
          <div className="pl-card-next-text">
            <div className="pl-card-kicker">NEXT · {clock(next.start)}</div>
            <div className="pl-card-next-title">{next.title}</div>
          </div>
          <button type="button" className="pl-record" aria-label={`Record ${next.title}`} onClick={recordNext}>
            <Icon name="record" size={14} /> Record
          </button>
        </div>
      )}
    </>
  );
}

/* ── Stats (C18) ────────────────────────────────────────────────────────── */

export function StatsPanel({ stats }: { stats: PlayerState['stats'] }) {
  const rows: [string, string][] = stats
    ? [
      ['Resolution', stats.resolution ?? '—'],
      ['Video codec', stats.videoCodec ?? '—'],
      ['Audio codec', stats.audioCodec ?? '—'],
      ['Frame rate', stats.fps ? `${stats.fps.toFixed(stats.fps % 1 ? 2 : 0)} fps` : '—'],
      ['Bitrate', stats.bitrateKbps ? `${(stats.bitrateKbps / 1000).toFixed(1)} Mbps` : '—'],
      ['Dropped frames', String(stats.droppedFrames)],
      ['Buffer', `${stats.bufferSecs.toFixed(1)}s`],
      ['Cache', `${(stats.cacheBytes / 1_048_576).toFixed(1)} MB`],
      ['Hardware decoder', stats.hwDecoder ?? 'software'],
    ]
    : [];
  return (
    <div {...chrome} className="pl-stats" role="group" aria-label="Playback statistics">
      {rows.length === 0
        ? <span>No figures yet — nothing is decoding.</span>
        : rows.map(([k, v]) => (
          <div key={k} style={{ display: 'contents' }}>
            <span className="pl-stats-key">{k}</span>
            <span>{v}</span>
          </div>
        ))}
    </div>
  );
}

/* ── Loading (C24) ──────────────────────────────────────────────────────── */

export function Waiting({ player }: { player: PlayerState }) {
  const words = player.reconnecting
    ? 'Reconnecting…'
    : player.status === 'loading' ? 'Opening…' : 'Buffering…';
  return (
    <div className="pl-waiting" role="status" aria-live="polite">
      <span aria-hidden className="pl-spinner" />
      {player.status === 'loading' && player.title && <div className="pl-waiting-title">{player.title}</div>}
      <div className="pl-waiting-words">{words}</div>
    </div>
  );
}

/* ── Errors (C25) ───────────────────────────────────────────────────────── */

const ACTIONS: Record<string, string> = {
  retry: 'Retry',
  tryAnotherSource: 'Try another source',
  openSettings: 'Open settings',
};

/**
 * A sentence, a likely cause, and only the actions that work.
 *
 * The host picks the actions per failure. `reportBroken` has no command behind it, so it
 * is left out rather than drawn as a button that does nothing.
 */
export function ErrorPanel({
  error, onSettings,
}: { error: NonNullable<PlayerState['error']>; onSettings: () => void }) {
  const actions = error.actions.filter((a) => a in ACTIONS);
  return (
    <div {...chrome} role="alert" className="pl-error">
      <div className="pl-error-icon"><Icon name="alert" size={32} /></div>
      <h2>{error.message}</h2>
      <p>{error.cause}</p>
      {actions.length > 0 && (
        <div className="pl-error-actions">
          {actions.map((a) => (
            <button
              key={a}
              type="button"
              className="pl-error-action"
              onClick={() => (a === 'openSettings' ? onSettings() : void control.playAgain())}
            >
              {ACTIONS[a]}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

/* ── The sleep warning ──────────────────────────────────────────────────── */

/**
 * Shown whatever the controls are doing: the timer fires precisely because nobody has
 * touched anything, so a warning that hid with the controls would never be seen.
 */
export function SleepWarning({ sleep }: { sleep: SleepTimer }) {
  if (!sleep.state.warning) return null;
  return (
    <div {...chrome} data-testid="sleep-warning" role="alert" className="pl-sleep-warning">
      <span>{sleepVerb(sleep.state)} in {sleep.state.remainingSecs ?? 0}s</span>
      <button
        type="button"
        className="pl-btn pl-btn-sm"
        aria-label={`Keep watching for another ${SLEEP_EXTEND_MINUTES} minutes`}
        title="Keep watching"
        onClick={() => sleep.extend(SLEEP_EXTEND_MINUTES)}
      >
        <Icon name="play" size={16} />
      </button>
      <button type="button" className="pl-btn pl-btn-sm" aria-label="Cancel the sleep timer" title="Cancel" onClick={sleep.cancel}>
        <Icon name="close" size={16} />
      </button>
    </div>
  );
}
