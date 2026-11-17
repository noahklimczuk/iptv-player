/**
 * Watch settings (C13): subtitles, audio and picture, as tabs of one sheet over the
 * dock. Each list marks the current choice. The picture tab also holds the speed (on
 * demand only), the aspect ratio and the sleep timer.
 */
import { useCallback, useEffect, useState } from 'react';
import type { PlayerState, SleepAfter, SleepState } from '@shared/ipc';
import { Icon } from '@/components/Icon';
import { invoke, onSleepState } from '@/ipc';
import { report } from '@/lib/errors';
import * as control from './playerControl';
import { CHROME_ATTR } from './useChrome';
import {
  ASPECTS, SLEEP_EXTEND_MINUTES, SLEEP_MINUTES, SPEEDS, speedLabel, trackName,
} from './model';

export type MenuTab = 'subtitles' | 'audio' | 'playback';

const TABS: [MenuTab, string][] = [['subtitles', 'Subtitles'], ['audio', 'Audio'], ['playback', 'Picture']];

export function WatchMenu({
  tab, onTab, onClose, player, sleep,
}: {
  tab: MenuTab;
  onTab: (t: MenuTab) => void;
  onClose: () => void;
  player: PlayerState;
  sleep: SleepTimer;
}) {
  return (
    <div {...{ [CHROME_ATTR]: '' }} className="pl-sheet" role="group" aria-label="Watch settings">
      <div className="pl-sheet-head">
        <div className="dl-display pl-sheet-title">Watch settings</div>
        <button type="button" className="pl-btn pl-btn-sm" aria-label="Close watch settings" title="Close" onClick={onClose}>
          <Icon name="close" size={18} />
        </button>
      </div>
      <div className="dl-seg pl-tabs" role="tablist" aria-label="Watch settings">
        {TABS.map(([key, label]) => (
          <button
            key={key}
            type="button"
            role="tab"
            aria-selected={tab === key}
            data-active={tab === key}
            onClick={() => onTab(key)}
          >
            {label}
          </button>
        ))}
      </div>
      {tab === 'subtitles' && <Tracks kind="subtitles" player={player} />}
      {tab === 'audio' && <Tracks kind="audio" player={player} />}
      {tab === 'playback' && <Picture player={player} sleep={sleep} />}
    </div>
  );
}

function Tracks({ kind, player }: { kind: 'audio' | 'subtitles'; player: PlayerState }) {
  const tracks = kind === 'audio' ? player.audioTracks : player.subtitleTracks;
  const active = kind === 'audio' ? player.activeAudioTrack : player.activeSubtitleTrack;
  return (
    <div role="group" aria-label={kind === 'audio' ? 'Audio track' : 'Subtitles'} className="pl-list">
      {kind === 'subtitles' && (
        <Choice label="Off" active={active === null} onPick={() => control.setSubtitleTrack(null)} />
      )}
      {tracks.map((t) => (
        <Choice
          key={t.id}
          label={trackName(t)}
          detail={[t.language, t.codec, t.channels].filter(Boolean).join(' · ')}
          active={active === t.id}
          onPick={() => (kind === 'audio' ? control.setAudioTrack(t.id) : control.setSubtitleTrack(t.id))}
        />
      ))}
      {kind === 'audio' && tracks.length === 0 && <p className="pl-empty">This stream has one audio track.</p>}
    </div>
  );
}

function Picture({ player, sleep }: { player: PlayerState; sleep: SleepTimer }) {
  return (
    <div data-testid="playback-panel">
      {/* Live arrives at the rate it arrives; faster would only run out of buffer. */}
      {!player.isLive && (
        <>
          <Heading>Speed</Heading>
          <div className="pl-chips" role="group" aria-label="Speed">
            {SPEEDS.map((s) => (
              <button
                key={s}
                type="button"
                className="pl-chip"
                aria-pressed={player.speed === s}
                onClick={() => control.setSpeed(s)}
              >
                {speedLabel(s)}
              </button>
            ))}
          </div>
        </>
      )}

      <Heading>Aspect ratio</Heading>
      <div role="group" aria-label="Aspect ratio" className="pl-list">
        {ASPECTS.map((a) => (
          <Choice
            key={a.value}
            label={a.label}
            detail={a.detail}
            active={player.aspect === a.value}
            onPick={() => control.setAspect(a.value)}
          />
        ))}
      </div>

      <Heading>Sleep timer</Heading>
      {sleep.state.armed ? (
        <div className="pl-sleep">
          <div data-testid="sleep-armed">{sleepVerb(sleep.state)} in {sleepLeft(sleep.state)}</div>
          <div className="pl-chips">
            <button type="button" className="pl-chip" onClick={() => sleep.extend(SLEEP_EXTEND_MINUTES)}>
              +{SLEEP_EXTEND_MINUTES} min
            </button>
            <button type="button" className="pl-chip" onClick={sleep.cancel}>Off</button>
          </div>
        </div>
      ) : (
        <div className="pl-chips">
          {SLEEP_MINUTES.map((m) => (
            <button key={m} type="button" className="pl-chip" onClick={() => sleep.set({ minutes: m })}>
              {m} min
            </button>
          ))}
          {/* Only what can be answered: live has a programme, on demand a runtime. */}
          {player.isLive ? (
            <button type="button" className="pl-chip" onClick={() => sleep.set('endOfProgramme')}>
              End of programme
            </button>
          ) : player.durationSecs > 0 && (
            <button type="button" className="pl-chip" onClick={() => sleep.set('endOfEpisode')}>
              End of {player.itemKind === 'episode' ? 'episode' : 'film'}
            </button>
          )}
        </div>
      )}
    </div>
  );
}

function Heading({ children }: { children: React.ReactNode }) {
  return <div className="pl-heading">{children}</div>;
}

function Choice({
  label, detail, active, onPick,
}: { label: string; detail?: string; active: boolean; onPick: () => void }) {
  return (
    <button type="button" aria-pressed={active} className="pl-choice" data-active={active} onClick={onPick}>
      <span className="pl-choice-text">
        <span className="pl-choice-label">{label}</span>
        {detail && <span className="pl-choice-detail">{detail}</span>}
      </span>
      <span aria-hidden className="pl-check"><Icon name="check" size={14} /></span>
    </button>
  );
}

/* ── The sleep timer ────────────────────────────────────────────────────── */

/**
 * The sleep timer, as the host keeps it (README §13).
 *
 * Every number comes from the host, which pushes `sleep.state` while a timer is armed.
 * Counting here would drift: the deadline is on the host's clock, and the whole point
 * is that nobody is touching the window.
 */
export function useSleepTimer() {
  const [state, setState] = useState<SleepState>({ armed: null, remainingSecs: null, warning: false });
  useEffect(() => {
    let live = true;
    invoke('sleep.state').then((s) => { if (live) setState(s); }).catch(() => undefined);
    const off = onSleepState(setState);
    return () => { live = false; off(); };
  }, []);
  const set = useCallback((after: SleepAfter) => {
    invoke('sleep.set', { after, action: 'stopPlayback' }).then(setState)
      .catch(report('Could not set the sleep timer'));
  }, []);
  const cancel = useCallback(() => {
    invoke('sleep.cancel').then(setState).catch(report('Could not cancel the sleep timer'));
  }, []);
  const extend = useCallback((minutes: number) => {
    invoke('sleep.extend', { minutes }).then(setState).catch(report('Could not extend the sleep timer'));
  }, []);
  return { state, set, cancel, extend };
}

export type SleepTimer = ReturnType<typeof useSleepTimer>;

export function sleepVerb(state: SleepState): string {
  switch (state.armed?.action) {
    case 'quitApp': return 'Closing Aurora';
    case 'shutDownPc': return 'Shutting down';
    default: return 'Stopping';
  }
}

function sleepLeft(state: SleepState): string {
  const left = state.remainingSecs ?? 0;
  return left >= 120 ? `${Math.round(left / 60)} min` : `${left}s`;
}
