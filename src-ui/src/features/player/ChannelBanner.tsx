/**
 * The set-top-box channel banner (README §7.2): number, logo, name, current programme
 * with a progress bar and time range, and "Next: …". Appears on every zap, then fades.
 */
import { AnimatePresence, motion } from 'framer-motion';
import type { Channel } from '@shared/ipc';
import { Badge, ChannelTile, ProgressBar } from '@/components/Primitives';
import { useCommand } from '@/hooks/useCommand';
import { progressPct } from '@/lib/format';
import { useClockTime } from '@/hooks/useClockTime';

export function ChannelBanner({
  channel, liftForOsd = false,
}: {
  channel: Channel | null;
  /** Raise the banner clear of the player's transport bar when the OSD is showing. */
  liftForOsd?: boolean;
}) {
  // Bound to the 12/24-hour preference, shadowing the pure import (R-136).
  const clockTime = useClockTime();
  const { data } = useCommand('epg.nowNext', { channelId: channel?.id ?? 0 }, [channel?.id]);
  const now = Math.floor(Date.now() / 1000);
  const pct = data?.now
    ? progressPct(now - data.now.start, data.now.stop - data.now.start)
    : 0;

  return (
    <AnimatePresence>
      {channel && (
        <motion.div
          data-testid="channel-banner"
          initial={{ opacity: 0, y: liftForOsd ? -20 : 30 }} animate={{ opacity: 1, y: 0 }}
          exit={{ opacity: 0, y: liftForOsd ? -20 : 30 }}
          transition={{ duration: 0.22, ease: [0.16, 1, 0.3, 1] }}
          style={{
            // Daylight: a white card like the player's own. Over the player it sits at
            // the top centre, between the channel badge and the chips, where it cannot
            // cover the On now card or the dock; elsewhere it is at the foot of the page.
            position: 'fixed', left: 0, right: 0,
            ...(liftForOsd ? { top: 24 } : { bottom: 'var(--sp-6)' }),
            zIndex: 160, pointerEvents: 'none',
            background: 'var(--dock-bg)', color: 'var(--dock-text)',
            borderRadius: 26, boxShadow: 'var(--shadow-dock)',
            padding: '14px 22px 14px 14px',
            display: 'grid', gridTemplateColumns: 'auto auto 1fr auto', gap: 16,
            alignItems: 'center', width: liftForOsd ? 'min(560px, 40vw)' : 'min(760px, 90vw)',
            margin: '0 auto',
          }}
        >
          <ChannelTile name={channel.name} number={channel.number} logo={channel.logo} size={52} />
          <div
            className="dl-display"
            style={{
              fontSize: 'var(--fs-2xl)', color: 'var(--accent)',
              fontVariantNumeric: 'tabular-nums', lineHeight: 1,
            }}
          >
            {channel.number}
          </div>

          <div style={{ minWidth: 0 }}>
            <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 3 }}>
              <strong
                style={{
                  fontSize: 'var(--fs-lg)', fontWeight: 800, whiteSpace: 'nowrap',
                  overflow: 'hidden', textOverflow: 'ellipsis',
                }}
              >
                {channel.name}
              </strong>
              {channel.quality && <Badge tone="neutral">{channel.quality}</Badge>}
              {data?.now?.isLive && <Badge tone="live">● Live</Badge>}
            </div>

            {data?.now ? (
              <>
                <div
                  style={{
                    fontSize: 'var(--fs-md)', fontWeight: 600, whiteSpace: 'nowrap',
                    overflow: 'hidden', textOverflow: 'ellipsis',
                  }}
                >
                  {data.now.title}
                </div>
                <div
                  style={{
                    display: 'flex', alignItems: 'center', gap: 'var(--sp-3)', marginTop: 5,
                  }}
                >
                  <span
                    style={{
                      fontSize: 'var(--fs-xs)', fontWeight: 700, color: 'var(--dock-muted)',
                      fontVariantNumeric: 'tabular-nums',
                    }}
                  >
                    {clockTime(data.now.start)}–{clockTime(data.now.stop)}
                  </span>
                  <div style={{ flex: 1, maxWidth: 280 }}>
                    <ProgressBar percent={pct} height={6} />
                  </div>
                </div>
                {data.next && (
                  <div
                    style={{
                      marginTop: 5, fontSize: 'var(--fs-xs)', fontWeight: 700,
                      color: 'var(--dock-muted)',
                      whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis',
                    }}
                  >
                    Next: {clockTime(data.next.start)} {data.next.title}
                  </div>
                )}
              </>
            ) : (
              <div style={{ fontSize: 'var(--fs-sm)', color: 'var(--dock-muted)' }}>
                No guide data for this channel
              </div>
            )}
          </div>

          <div
            style={{
              fontSize: 'var(--fs-xl)', fontWeight: 800, color: 'var(--dock-text)',
              fontVariantNumeric: 'tabular-nums',
            }}
          >
            {clockTime(now)}
          </div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}

/** Direct channel-number entry overlay (README §7.2): "2 _ _". */
export function DigitEntry({ digits }: { digits: string }) {
  return (
    <AnimatePresence>
      {digits && (
        <motion.div
          initial={{ opacity: 0, scale: 0.9 }} animate={{ opacity: 1, scale: 1 }}
          exit={{ opacity: 0, scale: 0.9 }} transition={{ duration: 0.12 }}
          role="status"
          aria-live="polite"
          aria-label={`Channel ${digits}`}
          style={{
            position: 'fixed', top: 'var(--sp-7)', right: 'var(--sp-7)', zIndex: 170,
            background: 'var(--dock-bg)', color: 'var(--accent)',
            border: 'none', borderRadius: 26,
            padding: 'var(--sp-4) var(--sp-6)', boxShadow: 'var(--shadow-dock)',
            fontFamily: 'var(--font-display)',
            fontSize: 'var(--fs-3xl)', fontWeight: 800, letterSpacing: '0.12em',
            fontVariantNumeric: 'tabular-nums', pointerEvents: 'none',
          }}
        >
          {digits.padEnd(3, '–')}
        </motion.div>
      )}
    </AnimatePresence>
  );
}
