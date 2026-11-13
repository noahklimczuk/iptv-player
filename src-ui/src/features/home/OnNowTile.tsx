/**
 * Home's "On now" tile: the viewer's favourite channels and what they are airing.
 *
 * Built from the same two queries Live TV already makes -- `channels.list` for the
 * favourites and `epg.nowNextMany` through `useNowNext` -- so it adds no command of its
 * own. Five channels, because a bento tile is a glance and not a guide.
 */
import { useEffect, useState } from 'react';
import type { Channel } from '@shared/ipc';
import { ChannelTile, ProgressBar } from '@/components/Primitives';
import { useCommand } from '@/hooks/useCommand';
import { useClockTime } from '@/hooks/useClockTime';
import { useNowNext } from '@/features/live/useNowNext';
import { useProfile } from '@/state/profile';

const SHOWN = 5;

export function OnNowTile({ onTune }: { onTune?: (channel: Channel) => void }) {
  const profileId = useProfile((s) => s.active?.id ?? 1);
  const clockTime = useClockTime();
  const { data: channels } = useCommand(
    'channels.list',
    { favoritesOnly: true, profileId },
    [profileId],
  );
  const shown = (channels ?? []).filter((c) => !c.hidden).slice(0, SHOWN);
  const guide = useNowNext(shown.map((c) => c.id));

  // Progress is about the clock, so it has to move on its own.
  const [now, setNow] = useState(() => Date.now() / 1000);
  useEffect(() => {
    const t = window.setInterval(() => setNow(Date.now() / 1000), 30_000);
    return () => window.clearInterval(t);
  }, []);

  return (
    <section
      aria-labelledby="home-on-now"
      className="dl-panel"
      style={{ padding: '24px 24px 20px', display: 'flex', flexDirection: 'column', minWidth: 0 }}
    >
      <div style={{ display: 'flex', alignItems: 'center', marginBottom: 14 }}>
        <h2
          id="home-on-now"
          className="dl-display"
          style={{ margin: 0, fontSize: 'var(--fs-2xl)', color: 'var(--text)' }}
        >
          On now
        </h2>
        <span className="dl-chip" data-tone="live" style={{ marginLeft: 'auto' }}>
          <span
            aria-hidden
            style={{ width: 7, height: 7, borderRadius: '50%', background: 'currentColor' }}
          />
          LIVE
        </span>
      </div>

      {channels && shown.length === 0 && (
        <p style={{ margin: 'auto 0', color: 'var(--text-muted)', fontSize: 'var(--fs-sm)' }}>
          Heart a channel in Live TV and it will show up here with what it is airing.
        </p>
      )}

      <div style={{ display: 'grid', gap: 10 }}>
        {shown.map((channel) => {
          const prog = guide.get(channel.id)?.now ?? null;
          const pct = prog
            ? ((now - prog.start) / Math.max(1, prog.stop - prog.start)) * 100
            : 0;
          return (
            <button
              key={channel.id}
              type="button"
              onClick={() => onTune?.(channel)}
              aria-label={`Watch ${channel.name}${prog ? `, ${prog.title}` : ''}`}
              className="dl-press"
              style={{
                display: 'grid', gridTemplateColumns: '48px minmax(0, 0.9fr) minmax(0, 1.1fr)',
                alignItems: 'center', gap: 14, padding: 0, border: 'none',
                background: 'transparent', textAlign: 'left', cursor: 'pointer',
                color: 'var(--text)', borderRadius: 'var(--r-md)',
              }}
            >
              <ChannelTile name={channel.name} number={channel.number} logo={channel.logo} />
              <span style={{ minWidth: 0 }}>
                <span
                  style={{
                    display: 'block', fontWeight: 800, fontSize: 'var(--fs-md)',
                    whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis',
                  }}
                >
                  {channel.name}
                </span>
                <span
                  style={{
                    display: 'block', fontSize: 'var(--fs-sm)', color: 'var(--text-muted)',
                    whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis',
                  }}
                >
                  {prog?.title ?? 'No guide'}
                </span>
              </span>
              <span style={{ display: 'grid', gap: 8 }}>
                <span
                  style={{
                    justifySelf: 'end', fontSize: 'var(--fs-xs)', fontWeight: 700,
                    color: 'var(--text-muted)', fontVariantNumeric: 'tabular-nums',
                  }}
                >
                  {prog ? `${clockTime(prog.start)}–${clockTime(prog.stop)}` : ''}
                </span>
                <ProgressBar percent={pct} height={6} />
              </span>
            </button>
          );
        })}
      </div>
    </section>
  );
}
