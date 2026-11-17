/**
 * The dock: one capsule at the bottom with everything you press (C6–C14).
 *
 * Every button has a name a screen reader says and a focus ring a keyboard can see
 * (C33). ← and → move between buttons while a button has focus; anywhere else they seek,
 * which is the global key map's job (C32).
 */
import type { Episode, PlayerState } from '@shared/ipc';
import { Icon, type IconName } from '@/components/Icon';
import type { NowNext } from '@/features/live/useNowNext';
import * as control from './playerControl';
import { Timeline } from './Scrub';
import { CHROME_ATTR } from './useChrome';
import { MAX_VOLUME, behindLive, canSkip } from './model';
import type { MenuTab } from './WatchMenu';

export function Dock({
  player, guide, menu, onMenu, fullscreen, onFullscreen, onGuide, nextEpisode, onPlayNext, onKeyWake,
}: {
  player: PlayerState;
  guide: NowNext | null;
  menu: MenuTab | null;
  onMenu: (tab: MenuTab) => void;
  fullscreen: boolean;
  onFullscreen: () => void;
  onGuide: () => void;
  nextEpisode: Episode | null;
  onPlayNext?: () => void;
  onKeyWake: () => void;
}) {
  const playing = player.status === 'playing' || player.status === 'buffering';
  const behind = behindLive(player);
  const skip = (secs: number) => (e: React.MouseEvent) => control.seekBy(e.shiftKey ? secs * 3 : secs);

  /** C33: arrows between buttons, and nothing else hears them. */
  const roving = (e: React.KeyboardEvent<HTMLDivElement>) => {
    if (e.key !== 'ArrowLeft' && e.key !== 'ArrowRight') return;
    const here = e.target as HTMLElement;
    if (here.tagName !== 'BUTTON') return;
    const buttons = Array.from(e.currentTarget.querySelectorAll<HTMLButtonElement>('button:not([disabled])'));
    const i = buttons.indexOf(here as HTMLButtonElement);
    if (i < 0) return;
    e.preventDefault();
    e.stopPropagation();
    onKeyWake();
    const next = buttons[(i + (e.key === 'ArrowRight' ? 1 : buttons.length - 1)) % buttons.length];
    next?.focus();
  };

  return (
    // The keys are a convenience on top of Tab, which reaches everything here.
    // eslint-disable-next-line jsx-a11y/no-static-element-interactions
    <div {...{ [CHROME_ATTR]: '' }} className="pl-dock" onKeyDown={roving}>
      <DockButton
        icon={playing ? 'pause' : 'play'}
        label={playing ? 'Pause' : 'Play'}
        keys="Space"
        primary
        onClick={control.togglePause}
      />
      {canSkip(player) && (
        <>
          <DockButton icon="back10" label="Back 10 seconds" keys="← · Shift for 30" onClick={skip(-10)} />
          <DockButton icon="forward10" label="Forward 10 seconds" keys="→ · Shift for 30" onClick={skip(10)} />
        </>
      )}
      {player.isLive && behind > 0 && (
        <button type="button" className="pl-btn pl-live-btn" title="Back to live (T)" onClick={control.backToLive}>
          <span aria-hidden className="pl-dot" /> Back to live
        </button>
      )}
      {nextEpisode && onPlayNext && (
        <DockButton
          icon="skip"
          label={`Next episode: S${nextEpisode.season} E${nextEpisode.episode}`}
          onClick={onPlayNext}
        />
      )}

      <div className="pl-dock-timeline">
        <Timeline player={player} guide={guide} />
      </div>

      <DockButton
        icon={player.muted ? 'volumeOff' : 'volume'}
        label={player.muted ? 'Unmute' : 'Mute'}
        keys="M"
        onClick={control.toggleMute}
      />
      <input
        type="range"
        className="dl-range pl-volume"
        aria-label="Volume"
        aria-valuetext={player.muted ? `Muted (${player.volume}%)` : `${player.volume}%`}
        title={`Volume ${player.volume}%`}
        min={0}
        max={MAX_VOLUME}
        step={1}
        value={player.volume}
        onChange={(e) => control.setVolume(Number(e.target.value))}
        style={{
          ['--fill' as string]: `${player.muted ? 0 : (player.volume / MAX_VOLUME) * 100}%`,
          ['--buf' as string]: `${(player.volume / MAX_VOLUME) * 100}%`,
        }}
      />
      {player.isLive && <DockButton icon="grid" label="TV Guide" keys="G" onClick={onGuide} />}
      <DockButton icon="subtitles" label="Subtitles" active={menu === 'subtitles'} onClick={() => onMenu('subtitles')} />
      <DockButton icon="audio" label="Audio track" active={menu === 'audio'} onClick={() => onMenu('audio')} />
      <DockButton icon="settings" label="Playback settings" active={menu === 'playback'} onClick={() => onMenu('playback')} />
      <DockButton
        icon="fullscreen"
        label={fullscreen ? 'Leave fullscreen' : 'Fullscreen'}
        keys="F"
        active={fullscreen}
        onClick={onFullscreen}
      />
    </div>
  );
}

function DockButton({
  icon, label, keys, active, primary, onClick,
}: {
  icon: IconName;
  label: string;
  keys?: string;
  active?: boolean;
  primary?: boolean;
  onClick: (e: React.MouseEvent) => void;
}) {
  return (
    <button
      type="button"
      className={primary ? 'pl-btn pl-primary' : 'pl-btn'}
      aria-label={label}
      aria-pressed={active}
      data-active={active || undefined}
      title={keys ? `${label} (${keys})` : label}
      onClick={onClick}
    >
      <Icon name={icon} size={primary ? 30 : 22} filled={primary} />
    </button>
  );
}
