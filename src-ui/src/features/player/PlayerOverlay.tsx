/**
 * The player (docs/PLAYER.md §3): the controls over the picture.
 *
 * The picture itself is not here. Under the native host mpv draws into a window behind
 * the page, and this overlay is transparent while there is a frame to see through to
 * (C27, C29). In a browser there is no surface and a stand-in is drawn instead.
 *
 * What this owns: when the controls show (`useChrome`), which menu is open, clicks and
 * the wheel on the picture. Keys are the global key map's (`useHotkeys`); this only
 * wakes on them, and takes Escape first while a menu is open.
 */
import { useCallback, useEffect, useRef, useState } from 'react';
import type { Episode, PlayerState } from '@shared/ipc';
import { Icon } from '@/components/Icon';
import { useCommand } from '@/hooks/useCommand';
import { hasVideoSurface } from '@/ipc';
import { useUi } from '@/state/ui';
import * as control from './playerControl';
import { Dock } from './Dock';
import { ErrorPanel, LiveCards, SleepWarning, StatsPanel, TopBar, Waiting } from './Panels';
import { CHROME_ATTR, useChrome } from './useChrome';
import { WatchMenu, useSleepTimer, type MenuTab } from './WatchMenu';
import { DOUBLE_CLICK_MS, isWaiting, showingPicture, wheelPixels, wheelVolume } from './model';
import './player.css';

export { behindLive, showingPicture } from './model';

export function PlayerOverlay({
  player, onClose, onGuide, onSettings, nextEpisode, onPlayNext, fullscreen, onFullscreen,
}: {
  player: PlayerState;
  onClose: () => void;
  onGuide: () => void;
  /** The error panel's "Open settings", which is where a bad provider is fixed. */
  onSettings: () => void;
  nextEpisode?: Episode | null;
  onPlayNext?: () => void;
  fullscreen: boolean;
  onFullscreen: () => void;
}) {
  const [menu, setMenu] = useState<MenuTab | null>(null);
  const chrome = useChrome({ paused: player.status === 'paused', onHide: () => setMenu(null) });
  const { setMenuOpen, wake } = chrome;
  useEffect(() => setMenuOpen(menu !== null), [menu, setMenuOpen]);

  const statsOpen = useUi((s) => s.playerStats);
  const sleep = useSleepTimer();

  // Escape closes an open menu before it does anything else. Capture phase, so the
  // global key map, which would leave fullscreen or the player, never hears it.
  const menuOpen = useRef(false);
  menuOpen.current = menu !== null;
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Escape' || !menuOpen.current) return;
      e.stopPropagation();
      e.preventDefault();
      setMenu(null);
    };
    window.addEventListener('keydown', onKey, true);
    return () => window.removeEventListener('keydown', onKey, true);
  }, []);

  // The stats panel is a property of this viewing, not of the app.
  useEffect(() => () => useUi.getState().togglePlayerStats(false), []);

  /*
   * The wheel turns the volume over the picture (C12). A native listener, because
   * React's wheel listener is passive and the page behind must not scroll. Travel is
   * carried between events, so a trackpad's stream of small deltas adds up to notches.
   */
  const root = useRef<HTMLDivElement>(null);
  const carried = useRef(0);
  useEffect(() => {
    const el = root.current;
    if (!el) return;
    const onWheel = (e: WheelEvent) => {
      if (overScrollableOrChrome(e.target, el)) return;
      e.preventDefault();
      wake();
      const p = useUi.getState().player;
      if (!p) return;
      const next = wheelVolume(carried.current, wheelPixels(e.deltaY, e.deltaMode), p.volume);
      carried.current = next.carried;
      if (next.volume !== p.volume) control.setVolume(next.volume);
    };
    el.addEventListener('wheel', onWheel, { passive: false });
    return () => el.removeEventListener('wheel', onWheel);
  }, [wake]);

  /*
   * A click on the picture plays or pauses (C6); a double click toggles fullscreen
   * (C14). The single click waits to see whether a second is coming, or a double click
   * would pause, then resume, as well as going fullscreen.
   */
  const pending = useRef<number | undefined>(undefined);
  useEffect(() => () => window.clearTimeout(pending.current), []);
  const onPictureClick = useCallback((e: React.MouseEvent) => {
    if (!onPicture(e.target)) return;
    window.clearTimeout(pending.current);
    if (e.detail >= 2) return;
    pending.current = window.setTimeout(control.togglePause, DOUBLE_CLICK_MS);
  }, []);
  const onPictureDoubleClick = useCallback((e: React.MouseEvent) => {
    if (!onPicture(e.target)) return;
    window.clearTimeout(pending.current);
    onFullscreen();
  }, [onFullscreen]);

  const picture = showingPicture(player);
  const toggleMenu = (tab: MenuTab) => setMenu((m) => (m === tab ? null : tab));

  return (
    // Clicks here are the picture's; the keyboard reaches the same actions through the
    // global key map (Space, F), so this element takes no focus of its own.
    // eslint-disable-next-line jsx-a11y/click-events-have-key-events, jsx-a11y/no-static-element-interactions
    <div
      ref={root}
      data-player-overlay=""
      data-controls={chrome.visible ? 'shown' : 'hidden'}
      className="pl-root"
      {...chrome.root}
      onClick={onPictureClick}
      onDoubleClick={onPictureDoubleClick}
      style={{
        background: picture ? 'transparent' : 'var(--player-bg)',
        cursor: chrome.visible ? 'default' : 'none',
      }}
    >
      {!hasVideoSurface() && (
        <div aria-hidden className="pl-standin">
          <Icon name="tv" size={64} strokeWidth={1} />
          <div>Video surface (libmpv renders here on Windows)</div>
        </div>
      )}

      {isWaiting(player) && !player.error && <Waiting player={player} />}
      {player.error && <ErrorPanel error={player.error} onSettings={onSettings} />}
      <SleepWarning sleep={sleep} />

      {chrome.visible && (
        <Controls
          player={player}
          menu={menu}
          onMenu={toggleMenu}
          onTab={setMenu}
          onCloseMenu={() => setMenu(null)}
          statsOpen={statsOpen}
          sleep={sleep}
          onClose={onClose}
          onGuide={onGuide}
          fullscreen={fullscreen}
          onFullscreen={onFullscreen}
          nextEpisode={nextEpisode ?? null}
          onPlayNext={onPlayNext}
          onKeyWake={wake}
        />
      )}
    </div>
  );
}

/** The controls proper, mounted only while shown so nothing hidden takes focus. */
function Controls({
  player, menu, onMenu, onTab, onCloseMenu, statsOpen, sleep, onClose, onGuide, fullscreen,
  onFullscreen, nextEpisode, onPlayNext, onKeyWake,
}: {
  player: PlayerState;
  menu: MenuTab | null;
  onMenu: (tab: MenuTab) => void;
  onTab: (tab: MenuTab) => void;
  onCloseMenu: () => void;
  statsOpen: boolean;
  sleep: ReturnType<typeof useSleepTimer>;
  onClose: () => void;
  onGuide: () => void;
  fullscreen: boolean;
  onFullscreen: () => void;
  nextEpisode: Episode | null;
  onPlayNext?: () => void;
  onKeyWake: () => void;
}) {
  // Now and next for the channel on screen, asked again on a zap and once a minute.
  const minute = Math.floor(Date.now() / 60_000);
  const { data: guide } = useCommand(
    'epg.nowNext',
    { channelId: player.channelId ?? 0 },
    [player.channelId, minute],
    player.isLive && player.channelId != null,
  );

  return (
    <>
      <div aria-hidden className="pl-scrim" />
      <TopBar
        player={player}
        onBack={onClose}
        statsOpen={statsOpen}
        onStats={() => useUi.getState().togglePlayerStats()}
      />
      {statsOpen && <StatsPanel stats={player.stats} />}
      {player.isLive && player.channelId != null && !menu && (
        <LiveCards channelId={player.channelId} title={player.title} guide={guide ?? null} />
      )}
      <Dock
        player={player}
        guide={guide ?? null}
        menu={menu}
        onMenu={onMenu}
        fullscreen={fullscreen}
        onFullscreen={onFullscreen}
        onGuide={onGuide}
        nextEpisode={nextEpisode}
        onPlayNext={onPlayNext}
        onKeyWake={onKeyWake}
      />
      {menu && <WatchMenu tab={menu} onTab={onTab} onClose={onCloseMenu} player={player} sleep={sleep} />}
    </>
  );
}

/** Whether a click landed on the picture rather than on a control. */
function onPicture(target: EventTarget | null): boolean {
  const el = target instanceof Element ? target : null;
  return !!el && !el.closest(`[${CHROME_ATTR}], button, input, a, [role="alert"]`);
}

/** The wheel belongs to the controls and to anything that scrolls, not to the volume. */
function overScrollableOrChrome(target: EventTarget | null, stopAt: HTMLElement): boolean {
  let node = target instanceof Element ? target : null;
  while (node && node !== stopAt) {
    if (node.hasAttribute(CHROME_ATTR)) return true;
    if (node.scrollHeight > node.clientHeight) {
      const y = getComputedStyle(node).overflowY;
      if (y === 'auto' || y === 'scroll') return true;
    }
    node = node.parentElement;
  }
  return false;
}
