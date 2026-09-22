/**
 * The single OSD used by live TV, movies, and episodes alike (README C3).
 *
 * On Windows the page behind this is transparent and libmpv renders the video; the OSD
 * floats over it. In the browser the same markup renders over a placeholder surface.
 */
import { AnimatePresence, motion } from 'framer-motion';
import { useCallback, useEffect, useRef, useState } from 'react';
import type { PlayerState } from '@shared/ipc';
import { Badge, IconButton } from '@/components/Primitives';
import { Icon } from '@/components/Icon';
import { invoke } from '@/ipc';
import { clockTime, duration, progressPct } from '@/lib/format';

const HIDE_AFTER_MS = 3200;

export function PlayerOverlay({
  player, onClose, onGuide,
}: {
  player: PlayerState;
  onClose: () => void;
  onGuide: () => void;
}) {
  const [visible, setVisible] = useState(true);
  const [showStats, setShowStats] = useState(false);
  const [panel, setPanel] = useState<'audio' | 'subtitles' | null>(null);
  const hideTimer = useRef<number | undefined>(undefined);

  const bump = useCallback(() => {
    setVisible(true);
    window.clearTimeout(hideTimer.current);
    hideTimer.current = window.setTimeout(() => {
      setVisible(false);
      setPanel(null);
    }, HIDE_AFTER_MS);
  }, []);

  useEffect(() => {
    bump();
    return () => window.clearTimeout(hideTimer.current);
  }, [bump]);

  const playing = player.status === 'playing';
  const pct = player.isLive ? 100 : progressPct(player.positionSecs, player.durationSecs);

  return (
    <div
      onMouseMove={bump}
      onClick={bump}
      style={{
        position: 'fixed', inset: 0, zIndex: 150,
        // Opaque only where there is no real video surface behind (browser dev).
        background: 'radial-gradient(ellipse at center, #10101a 0%, #05050a 100%)',
        cursor: visible ? 'default' : 'none',
      }}
    >
      {/* Stand-in for the mpv surface. On Windows this element is fully transparent. */}
      <div
        aria-hidden
        style={{
          position: 'absolute', inset: 0, display: 'grid', placeItems: 'center',
          color: 'var(--text-faint)',
        }}
      >
        <div style={{ textAlign: 'center' }}>
          <Icon name="tv" size={64} strokeWidth={1} />
          <div style={{ marginTop: 12, fontSize: 'var(--fs-sm)' }}>
            Video surface (libmpv renders here on Windows)
          </div>
        </div>
      </div>

      {player.status === 'buffering' && (
        <div
          style={{
            position: 'absolute', top: '50%', left: '50%',
            transform: 'translate(-50%, -50%)', color: 'var(--text)',
          }}
        >
          Buffering…
        </div>
      )}

      {player.error && <ErrorPanel error={player.error} />}

      <AnimatePresence>
        {visible && (
          <>
            <motion.div
              initial={{ opacity: 0, y: -16 }} animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, y: -16 }} transition={{ duration: 0.18 }}
              style={{
                position: 'absolute', top: 0, left: 0, right: 0,
                padding: 'var(--sp-4) var(--sp-5)',
                display: 'flex', alignItems: 'center', gap: 'var(--sp-3)',
                background: 'linear-gradient(to bottom, rgb(0 0 0 / 0.75), transparent)',
              }}
            >
              <IconButton icon="chevronLeft" label="Back" onClick={onClose} />
              <div style={{ minWidth: 0 }}>
                <div
                  style={{
                    fontSize: 'var(--fs-lg)', fontWeight: 700, whiteSpace: 'nowrap',
                    overflow: 'hidden', textOverflow: 'ellipsis',
                  }}
                >
                  {player.title ?? 'Nothing playing'}
                </div>
                {player.subtitle && (
                  <div style={{ fontSize: 'var(--fs-sm)', color: 'var(--text-muted)' }}>
                    {player.subtitle}
                  </div>
                )}
              </div>
              {player.isLive && <Badge tone="live">● Live</Badge>}
              <div style={{ marginLeft: 'auto', display: 'flex', gap: 6 }}>
                <IconButton
                  icon="info" label="Playback stats"
                  active={showStats}
                  onClick={() => setShowStats((s) => !s)}
                />
                <IconButton icon="pip" label="Picture-in-picture" />
                <IconButton icon="record" label="Record" />
              </div>
            </motion.div>

            {showStats && player.stats && <StatsOverlay stats={player.stats} />}

            <motion.div
              initial={{ opacity: 0, y: 24 }} animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, y: 24 }} transition={{ duration: 0.18 }}
              style={{
                position: 'absolute', left: 0, right: 0, bottom: 0,
                padding: 'var(--sp-5)',
                background: 'linear-gradient(to top, rgb(0 0 0 / 0.9), transparent)',
              }}
            >
              <Scrubber player={player} pct={pct} />

              <div
                style={{
                  display: 'flex', alignItems: 'center', gap: 'var(--sp-2)',
                  marginTop: 'var(--sp-3)',
                }}
              >
                <IconButton
                  icon={playing ? 'pause' : 'play'} filled active size={46}
                  label={playing ? 'Pause' : 'Play'}
                  onClick={() => void invoke(playing ? 'player.pause' : 'player.resume')}
                />
                {!player.isLive && (
                  <>
                    <IconButton
                      icon="back10" label="Back 10 seconds"
                      onClick={() => void invoke('player.seek', { positionSecs: -10, relative: true })}
                    />
                    <IconButton
                      icon="forward10" label="Forward 10 seconds"
                      onClick={() => void invoke('player.seek', { positionSecs: 10, relative: true })}
                    />
                  </>
                )}
                <IconButton
                  icon={player.muted ? 'volumeOff' : 'volume'}
                  label={player.muted ? 'Unmute' : 'Mute'}
                  onClick={() => void invoke('player.setMuted', { muted: !player.muted })}
                />
                <input
                  type="range" min={0} max={200} value={player.volume}
                  aria-label="Volume"
                  onChange={(e) => void invoke('player.setVolume', { volume: Number(e.target.value) })}
                  style={{ width: 110, accentColor: 'var(--accent)' }}
                />
                <span
                  style={{
                    fontSize: 'var(--fs-xs)', color: 'var(--text-faint)', width: 38,
                    fontVariantNumeric: 'tabular-nums',
                  }}
                >
                  {player.volume}%
                </span>

                <div style={{ marginLeft: 'auto', display: 'flex', gap: 6 }}>
                  {player.isLive && (
                    <IconButton icon="grid" label="TV Guide" onClick={onGuide} />
                  )}
                  <IconButton
                    icon="audio" label="Audio track"
                    active={panel === 'audio'}
                    onClick={() => setPanel((p) => (p === 'audio' ? null : 'audio'))}
                  />
                  <IconButton
                    icon="subtitles" label="Subtitles"
                    active={panel === 'subtitles'}
                    onClick={() => setPanel((p) => (p === 'subtitles' ? null : 'subtitles'))}
                  />
                  <IconButton icon="settings" label="Playback settings" />
                  <IconButton icon="fullscreen" label="Fullscreen" />
                </div>
              </div>

              <AnimatePresence>
                {panel && (
                  <TrackPanel
                    kind={panel}
                    player={player}
                    onPick={(id) =>
                      void invoke(
                        panel === 'audio' ? 'player.setAudioTrack' : 'player.setSubtitleTrack',
                        { trackId: id as number },
                      )
                    }
                  />
                )}
              </AnimatePresence>
            </motion.div>
          </>
        )}
      </AnimatePresence>
    </div>
  );
}

function Scrubber({ player, pct }: { player: PlayerState; pct: number }) {
  if (player.isLive) {
    return (
      <div style={{ display: 'flex', alignItems: 'center', gap: 'var(--sp-3)' }}>
        <div
          style={{
            flex: 1, height: 4, borderRadius: 'var(--r-full)',
            background: 'linear-gradient(to right, var(--live), var(--live))',
          }}
        />
        <span style={{ fontSize: 'var(--fs-xs)', color: 'var(--text-muted)' }}>
          Live · {clockTime(Math.floor(Date.now() / 1000))}
        </span>
      </div>
    );
  }
  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: 'var(--sp-3)' }}>
      <span
        style={{
          fontSize: 'var(--fs-sm)', color: 'var(--text-muted)', width: 58,
          fontVariantNumeric: 'tabular-nums',
        }}
      >
        {duration(player.positionSecs)}
      </span>
      <input
        type="range" min={0} max={Math.max(1, player.durationSecs)}
        value={player.positionSecs}
        aria-label="Seek"
        onChange={(e) => void invoke('player.seek', { positionSecs: Number(e.target.value) })}
        style={{ flex: 1, accentColor: 'var(--accent)' }}
      />
      <span
        style={{
          fontSize: 'var(--fs-sm)', color: 'var(--text-muted)', width: 58,
          textAlign: 'right', fontVariantNumeric: 'tabular-nums',
        }}
      >
        {duration(player.durationSecs)}
      </span>
      <span style={{ display: 'none' }}>{pct}</span>
    </div>
  );
}

function TrackPanel({
  kind, player, onPick,
}: {
  kind: 'audio' | 'subtitles';
  player: PlayerState;
  onPick: (id: number | null) => void;
}) {
  const tracks = kind === 'audio' ? player.audioTracks : player.subtitleTracks;
  const active = kind === 'audio' ? player.activeAudioTrack : player.activeSubtitleTrack;

  return (
    <motion.div
      initial={{ opacity: 0, y: 8 }} animate={{ opacity: 1, y: 0 }} exit={{ opacity: 0, y: 8 }}
      transition={{ duration: 0.15 }}
      style={{
        position: 'absolute', right: 'var(--sp-5)', bottom: 84, width: 300,
        background: 'var(--bg-elevated)', border: '1px solid var(--border)',
        borderRadius: 'var(--r-lg)', padding: 'var(--sp-3)', boxShadow: 'var(--shadow-3)',
      }}
    >
      <div
        style={{
          fontSize: 'var(--fs-xs)', color: 'var(--text-faint)', fontWeight: 700,
          letterSpacing: '0.06em', marginBottom: 'var(--sp-2)', textTransform: 'uppercase',
        }}
      >
        {kind === 'audio' ? 'Audio track' : 'Subtitles'}
      </div>

      {kind === 'subtitles' && (
        <TrackOption
          label="Off" active={active === null} onClick={() => onPick(null)}
        />
      )}
      {tracks.map((t) => (
        <TrackOption
          key={t.id}
          label={t.title ?? t.language ?? `Track ${t.id}`}
          detail={[t.language, t.codec, t.channels].filter(Boolean).join(' · ')}
          active={active === t.id}
          onClick={() => onPick(t.id)}
        />
      ))}
    </motion.div>
  );
}

function TrackOption({
  label, detail, active, onClick,
}: { label: string; detail?: string; active: boolean; onClick: () => void }) {
  return (
    <button
      onClick={onClick}
      style={{
        display: 'flex', alignItems: 'center', gap: 'var(--sp-2)', width: '100%',
        padding: 'var(--sp-2) var(--sp-3)', borderRadius: 'var(--r-md)', border: 'none',
        background: active ? 'var(--surface-hover)' : 'transparent',
        color: 'inherit', cursor: 'pointer', textAlign: 'left',
      }}
    >
      <Icon
        name="check" size={15}
        style={{ opacity: active ? 1 : 0, color: 'var(--accent)' }}
      />
      <span style={{ flex: 1 }}>
        <span style={{ display: 'block', fontSize: 'var(--fs-sm)', fontWeight: 600 }}>
          {label}
        </span>
        {detail && (
          <span style={{ display: 'block', fontSize: 'var(--fs-xs)', color: 'var(--text-faint)' }}>
            {detail}
          </span>
        )}
      </span>
    </button>
  );
}

/** README §6.2: resolution, codecs, fps, bitrate, dropped frames, buffer, hw decoder. */
function StatsOverlay({ stats }: { stats: NonNullable<PlayerState['stats']> }) {
  const rows: [string, string][] = [
    ['Resolution', stats.resolution ?? '—'],
    ['Video codec', stats.videoCodec ?? '—'],
    ['Audio codec', stats.audioCodec ?? '—'],
    ['Frame rate', stats.fps ? `${stats.fps} fps` : '—'],
    ['Bitrate', stats.bitrateKbps ? `${(stats.bitrateKbps / 1000).toFixed(1)} Mbps` : '—'],
    ['Dropped frames', String(stats.droppedFrames)],
    ['Buffer', `${stats.bufferSecs.toFixed(1)}s`],
    ['Hardware decoder', stats.hwDecoder ?? 'software'],
  ];
  return (
    <div
      style={{
        position: 'absolute', top: 88, left: 'var(--sp-5)',
        background: 'rgb(0 0 0 / 0.78)', border: '1px solid var(--border)',
        borderRadius: 'var(--r-md)', padding: 'var(--sp-3) var(--sp-4)',
        fontFamily: 'ui-monospace, "Cascadia Code", Consolas, monospace',
        fontSize: 'var(--fs-xs)', display: 'grid',
        gridTemplateColumns: 'auto auto', gap: '3px var(--sp-4)',
      }}
    >
      {rows.map(([k, v]) => (
        <div key={k} style={{ display: 'contents' }}>
          <span style={{ color: 'var(--text-faint)' }}>{k}</span>
          <span style={{ color: 'var(--text)' }}>{v}</span>
        </div>
      ))}
    </div>
  );
}

/** README §17: a human sentence, a likely cause, and at least one action. */
function ErrorPanel({ error }: { error: NonNullable<PlayerState['error']> }) {
  const LABELS: Record<string, string> = {
    retry: 'Retry',
    tryAnotherSource: 'Try another source',
    reportBroken: 'Report stream as broken',
    openSettings: 'Open settings',
  };
  return (
    <div
      role="alert"
      style={{
        position: 'absolute', top: '50%', left: '50%', transform: 'translate(-50%,-50%)',
        maxWidth: 460, textAlign: 'center', background: 'var(--bg-elevated)',
        border: '1px solid var(--border-strong)', borderRadius: 'var(--r-lg)',
        padding: 'var(--sp-6)', boxShadow: 'var(--shadow-4)',
      }}
    >
      <div style={{ color: 'var(--danger)', marginBottom: 'var(--sp-3)' }}>
        <Icon name="info" size={32} style={{ margin: '0 auto' }} />
      </div>
      <h2 style={{ margin: '0 0 var(--sp-2)', fontSize: 'var(--fs-lg)' }}>{error.message}</h2>
      <p style={{ margin: '0 0 var(--sp-4)', color: 'var(--text-muted)', fontSize: 'var(--fs-sm)' }}>
        {error.cause}
      </p>
      <div style={{ display: 'flex', gap: 'var(--sp-2)', justifyContent: 'center', flexWrap: 'wrap' }}>
        {error.actions.map((a) => (
          <button
            key={a}
            style={{
              padding: '8px 16px', borderRadius: 'var(--r-md)', cursor: 'pointer',
              border: '1px solid var(--border-strong)', background: 'var(--surface)',
              color: 'var(--text)', fontWeight: 600,
            }}
          >
            {LABELS[a] ?? a}
          </button>
        ))}
      </div>
    </div>
  );
}
