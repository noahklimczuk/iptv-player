import { useCallback, useEffect, useMemo, useState } from 'react';
import { NavLink, Route, Routes, useLocation, useNavigate } from 'react-router-dom';
import type { CatalogItem, SearchHit } from '@shared/ipc';
import { DetailModal } from '@/components/DetailModal';
import { Icon, type IconName } from '@/components/Icon';
import { BrowsePage } from '@/features/browse/BrowsePage';
import { RecordingsPage } from '@/features/dvr/RecordingsPage';
import { GuidePage } from '@/features/guide/GuidePage';
import { HomePage } from '@/features/home/HomePage';
import { PlaylistPage } from '@/features/playlist/PlaylistPage';
import { LivePage } from '@/features/live/LivePage';
import { ChannelBanner, DigitEntry } from '@/features/player/ChannelBanner';
import { SkipButton } from '@/features/player/SkipButton';
import { UpNextCard } from '@/features/player/UpNextCard';
import { PlayerOverlay } from '@/features/player/PlayerOverlay';
import { CommandPalette } from '@/features/search/CommandPalette';
import { ProfilePicker } from '@/features/profiles/ProfilePicker';
import { SetupWizard } from '@/features/setup/SetupWizard';
import { SettingsPage } from '@/features/settings/SettingsPage';
import { useCommand } from '@/hooks/useCommand';
import { useEpisodeAids } from '@/hooks/useEpisodeAids';
import { useHotkeys } from '@/hooks/useHotkeys';
import { useZapper } from '@/hooks/useZapper';
import { invoke } from '@/ipc';
import { useProfile } from '@/state/profile';
import { bindPlayerState, useUi } from '@/state/ui';

const NAV: { to: string; icon: IconName; label: string }[] = [
  { to: '/', icon: 'home', label: 'Home' },
  { to: '/live', icon: 'tv', label: 'Live TV' },
  { to: '/guide', icon: 'grid', label: 'Guide' },
  { to: '/movies', icon: 'film', label: 'Movies' },
  { to: '/series', icon: 'stack', label: 'Series' },
  { to: '/recordings', icon: 'record', label: 'Recordings' },
  { to: '/playlist', icon: 'layers', label: 'Playlist' },
  { to: '/settings', icon: 'settings', label: 'Settings' },
];

export default function App() {
  const navigate = useNavigate();
  const location = useLocation();
  const ui = useUi();
  const [playerOpen, setPlayerOpen] = useState(false);
  // Shown until a provider exists. `?setup` forces it for demos and tests.
  const [setupDone, setSetupDone] = useState(
    () => !new URLSearchParams(window.location.search).has('setup'),
  );

  const { data: providers } = useCommand('providers.list', undefined, []);
  const { data: channels } = useCommand('channels.list', {}, []);
  // Every tune path funnels through here, so digit entry and Ch+/Ch- surface the
  // player exactly like clicking a channel does.
  const zapper = useZapper(channels ?? [], { onTune: () => setPlayerOpen(true) });
  const tune = zapper.tune;

  useEffect(() => bindPlayerState(), []);

  const profile = useProfile();
  useEffect(() => { void profile.load(); }, [profile.load]);

  const play = useCallback(async (item: CatalogItem, episodeId?: number) => {
    ui.openDetail(null);
    if (item.kind === 'series') {
      const eps = await invoke('library.episodes', { seriesId: item.id });
      const target = episodeId ?? eps[0]?.id;
      if (target != null) await invoke('player.play', { kind: 'episode', id: target });
    } else {
      await invoke('player.play', { kind: 'movie', id: item.id });
    }
    setPlayerOpen(true);
  }, [ui]);

  /**
   * Play a past programme from its start.
   *
   * Not routed through the zapper: the zapper tunes a channel to its live edge, and
   * this is the opposite request. The error is surfaced rather than swallowed because
   * the three ways catch-up can refuse — no catch-up, outside the window, provider did
   * not say how — are things the viewer can act on.
   */
  const playCatchup = useCallback(
    async (channelId: number, start: number, stop: number) => {
      await invoke('player.playCatchup', { channelId, start, stop });
      setPlayerOpen(true);
    },
    [],
  );

  const playEpisode = useCallback(async (episodeId: number) => {
    await invoke('player.play', { kind: 'episode', id: episodeId });
    setPlayerOpen(true);
  }, []);

  const episode = useEpisodeAids(ui.player, (id) => void playEpisode(id));

  const onPick = useCallback((hit: SearchHit) => {
    // A programme hit carries the channel it is on, not the programme id: what the
    // viewer wants from one is to watch it, and only the channel can do that.
    if (hit.kind === 'channel' || hit.kind === 'programme') {
      const ch = (channels ?? []).find((c) => c.id === hit.refId);
      if (ch) {
        tune(ch);
        return;
      }
      // An upcoming programme on a channel we cannot resolve: the guide is where it
      // lives, which beats doing nothing.
      navigate('/guide');
      return;
    }
    if (hit.kind === 'movie' || hit.kind === 'series') {
      navigate(hit.kind === 'movie' ? '/movies' : '/series');
    }
  }, [channels, navigate, tune]);

  const handlers = useMemo(() => ({
    onDigit: (d: string) => ui.pushDigit(d),
    onChannelUp: () => zapper.step(1),
    onChannelDown: () => zapper.step(-1),
    onLastChannel: () => zapper.lastChannel(),
    onGuide: () => { setPlayerOpen(false); navigate('/guide'); },
    onPalette: () => ui.setPalette(true),
    onBack: () => {
      if (ui.detail) ui.openDetail(null);
      else if (playerOpen) setPlayerOpen(false);
    },
    onPlayPause: () => {
      const s = ui.player?.status;
      if (s) void invoke(s === 'playing' ? 'player.pause' : 'player.resume');
    },
    onSeek: (secs: number) => void invoke('player.seek', { positionSecs: secs, relative: true }),
    onVolume: (delta: number) =>
      void invoke('player.setVolume', { volume: (ui.player?.volume ?? 70) + delta }),
    onMute: () => void invoke('player.setMuted', { muted: !(ui.player?.muted ?? false) }),
    onFullscreen: () => void document.documentElement.requestFullscreen?.().catch(() => {}),
    onInfo: () => {},
    onNavigate: (to: string) => { setPlayerOpen(false); navigate(to); },
  }), [ui, zapper, navigate, playerOpen]);

  useHotkeys(handlers, !ui.paletteOpen);

  const bannerChannel = ui.banner
    ? (channels ?? []).find((c) => c.id === ui.banner!.channelId) ?? null
    : null;

  // Auto-hide the banner.
  useEffect(() => {
    if (!ui.banner) return;
    const t = window.setTimeout(() => ui.hideBanner(), 5000);
    return () => window.clearTimeout(t);
  }, [ui.banner, ui]);

  const needsSetup = !setupDone || providers?.length === 0;
  if (needsSetup) {
    return <SetupWizard onFinished={() => setSetupDone(true)} />;
  }

  // "Who's watching?" comes after setup — there is no point choosing a profile for
  // an empty library.
  if (profile.picking || !profile.active) {
    return <ProfilePicker />;
  }

  return (
    <div style={{ display: 'flex', height: '100%', background: 'var(--bg)' }}>
      <nav
        aria-label="Main"
        style={{
          width: 'var(--sidebar-w)', flexShrink: 0, background: 'var(--bg-elevated)',
          borderRight: '1px solid var(--border)', display: 'flex',
          flexDirection: 'column', alignItems: 'center', padding: 'var(--sp-4) 0',
          gap: 'var(--sp-2)', zIndex: 50,
        }}
      >
        <div
          style={{
            width: 38, height: 38, borderRadius: 'var(--r-md)', display: 'grid',
            placeItems: 'center', marginBottom: 'var(--sp-4)',
            background: 'linear-gradient(135deg, var(--accent), var(--accent-2))',
            color: '#fff', fontWeight: 900, fontSize: 18,
          }}
          title="Aurora TV"
        >
          A
        </div>

        {NAV.map((n) => (
          <NavLink
            key={n.to}
            to={n.to}
            end={n.to === '/'}
            onClick={() => setPlayerOpen(false)}
            style={({ isActive }) => ({
              width: 'calc(var(--sidebar-w) - 20px)', padding: 'var(--sp-2) 0',
              display: 'grid', placeItems: 'center', gap: 3, borderRadius: 'var(--r-md)',
              textDecoration: 'none', fontSize: 10, fontWeight: 600,
              color: isActive ? 'var(--text)' : 'var(--text-faint)',
              background: isActive ? 'var(--surface-hover)' : 'transparent',
              marginTop: n.to === '/settings' ? 'auto' : undefined,
            })}
          >
            <Icon name={n.icon} size={21} />
            {n.label}
          </NavLink>
        ))}
      </nav>

      <main style={{ flex: 1, overflowY: 'auto', overflowX: 'hidden', position: 'relative' }}>
        <TopBar
          onSearch={() => ui.setPalette(true)}
          title={titleFor(location.pathname)}
          profileName={profile.active.name}
          onSwitchProfile={() => profile.setPicking(true)}
        />
        <Routes>
          <Route path="/" element={<HomePage onOpen={ui.openDetail} onPlay={play} />} />
          <Route path="/live" element={<LivePage onTune={tune} />} />
          <Route path="/guide" element={<GuidePage onTune={tune} onCatchup={playCatchup} onSearch={(q) => ui.setPalette(true, q)} />} />
          <Route path="/movies" element={<BrowsePage mode="movies" onOpen={ui.openDetail} onPlay={play} />} />
          <Route path="/series" element={<BrowsePage mode="series" onOpen={ui.openDetail} onPlay={play} />} />
          <Route path="/recordings" element={<RecordingsPage />} />
          <Route path="/playlist" element={<PlaylistPage />} />
          <Route path="/settings" element={<SettingsPage />} />
        </Routes>
      </main>

      {playerOpen && ui.player && (
        <PlayerOverlay
          player={ui.player}
          onClose={() => setPlayerOpen(false)}
          onGuide={() => { setPlayerOpen(false); navigate('/guide'); }}
          nextEpisode={episode.aids?.nextEpisode ?? null}
          onPlayNext={episode.aids?.nextEpisode ? episode.playNext : undefined}
        />
      )}

      {playerOpen && (
        /* One stack above the transport bar. Skip Credits and Up Next can both be
           live during the credits, so they queue rather than overlap. */
        <div
          style={{
            position: 'fixed', right: 'var(--sp-6)', bottom: 148, zIndex: 166,
            display: 'flex', flexDirection: 'column', alignItems: 'flex-end',
            gap: 'var(--sp-3)', pointerEvents: 'none',
          }}
        >
          <div style={{ pointerEvents: 'auto' }}>
            <UpNextCard
              episode={episode.aids?.nextEpisode ?? null}
              visible={episode.upNextVisible}
              autoplay={episode.aids?.prefs.autoplayNext ?? true}
              onPlay={episode.playNext}
              onCancel={episode.dismissUpNext}
            />
          </div>
          <div style={{ pointerEvents: 'auto' }}>
            <SkipButton
              marker={episode.activeMarker}
              autoSkip={episode.autoSkip}
              onSkip={episode.skip}
            />
          </div>
        </div>
      )}

      <ChannelBanner channel={bannerChannel} liftForOsd={playerOpen} />
      <DigitEntry digits={ui.digits} />

      <DetailModal item={ui.detail} onClose={() => ui.openDetail(null)} onPlay={play} />

      <CommandPalette
        open={ui.paletteOpen}
        initialQuery={ui.paletteQuery}
        onClose={() => ui.setPalette(false)}
        onPick={onPick}
      />
    </div>
  );
}

function titleFor(path: string): string {
  const hit = NAV.find((n) => (n.to === '/' ? path === '/' : path.startsWith(n.to)));
  return hit?.label ?? 'Aurora TV';
}

function TopBar({
  title, onSearch, profileName, onSwitchProfile,
}: {
  title: string;
  onSearch: () => void;
  profileName: string;
  onSwitchProfile: () => void;
}) {
  const [clock, setClock] = useState(() => new Date());
  useEffect(() => {
    const t = window.setInterval(() => setClock(new Date()), 15_000);
    return () => window.clearInterval(t);
  }, []);

  return (
    <header
      style={{
        position: 'sticky', top: 0, zIndex: 40, height: 'var(--topbar-h)',
        display: 'flex', alignItems: 'center', gap: 'var(--sp-4)',
        padding: '0 var(--sp-6)',
        background: 'linear-gradient(to bottom, var(--bg) 40%, transparent)',
        pointerEvents: 'none',
      }}
    >
      <span style={{ fontWeight: 700, fontSize: 'var(--fs-md)', opacity: 0.85 }}>{title}</span>
      <div style={{ marginLeft: 'auto', display: 'flex', alignItems: 'center', gap: 'var(--sp-3)', pointerEvents: 'auto' }}>
        <button
          onClick={onSearch}
          aria-label="Search"
          style={{
            display: 'flex', alignItems: 'center', gap: 8, padding: '7px 14px',
            borderRadius: 'var(--r-full)', cursor: 'pointer',
            background: 'color-mix(in srgb, var(--surface) 75%, transparent)',
            border: '1px solid var(--border)', color: 'var(--text-muted)',
            backdropFilter: 'blur(12px)', fontSize: 'var(--fs-sm)',
          }}
        >
          <Icon name="search" size={15} />
          Search
          <kbd style={{ fontSize: 10, opacity: 0.7 }}>Ctrl K</kbd>
        </button>
        <span
          style={{
            fontSize: 'var(--fs-sm)', color: 'var(--text-muted)',
            fontVariantNumeric: 'tabular-nums',
          }}
        >
          {String(clock.getHours()).padStart(2, '0')}:{String(clock.getMinutes()).padStart(2, '0')}
        </span>
        <button
          onClick={onSwitchProfile}
          aria-label={`Switch profile (currently ${profileName})`}
          title={`Switch profile (currently ${profileName})`}
          style={{
            width: 32, height: 32, borderRadius: 'var(--r-md)', cursor: 'pointer',
            border: '1px solid var(--border)', background: 'var(--surface)',
            color: 'var(--text)', fontWeight: 700, fontSize: 'var(--fs-sm)',
          }}
        >
          {profileName.slice(0, 1).toUpperCase()}
        </button>
      </div>
    </header>
  );
}
