/**
 * The set-top-box channel banner (README §7.2): number, logo, name, current programme
 * with a progress bar and time range, and "Next: …". Appears on every zap, then fades.
 */
import { AnimatePresence, motion } from 'framer-motion';
import type { Channel } from '@shared/ipc';
import { Badge } from '@/components/Primitives';
import { useCommand } from '@/hooks/useCommand';
import { clockTime, progressPct } from '@/lib/format';

export function ChannelBanner({
  channel, liftForOsd = false,
}: {
  channel: Channel | null;
  /** Raise the banner clear of the player's transport bar when the OSD is showing. */
  liftForOsd?: boolean;
}) {
  const { data } = useCommand('epg.nowNext', { channelId: channel?.id ?? 0 }, [channel?.id]);
  const now = Math.floor(Date.now() / 1000);
  const pct = data?.now
    ? progressPct(now - data.now.start, data.now.stop - data.now.start)
    : 0;

  return (
    <AnimatePresence>
      {channel && (
        <motion.div
          initial={{ opacity: 0, y: 30 }} animate={{ opacity: 1, y: 0 }}
          exit={{ opacity: 0, y: 30 }} transition={{ duration: 0.22, ease: [0.16, 1, 0.3, 1] }}
          style={{
            position: 'fixed', left: 'var(--sp-6)', right: 'var(--sp-6)',
            bottom: liftForOsd ? 148 : 'var(--sp-6)',
            zIndex: 160, pointerEvents: 'none',
            background: 'rgb(8 8 12 / 0.9)', backdropFilter: 'blur(18px)',
            border: '1px solid var(--border)', borderRadius: 'var(--r-lg)',
            boxShadow: 'var(--shadow-4)', padding: 'var(--sp-4) var(--sp-5)',
            display: 'grid', gridTemplateColumns: 'auto auto 1fr auto', gap: 'var(--sp-4)',
            alignItems: 'center', maxWidth: 900, margin: '0 auto',
          }}
        >
          <div
            style={{
              fontSize: 'var(--fs-2xl)', fontWeight: 900, color: 'var(--accent)',
              fontVariantNumeric: 'tabular-nums', lineHeight: 1,
            }}
          >
            {channel.number}
          </div>
          {channel.logo && (
            <img
              src={channel.logo} alt=""
              style={{ width: 52, height: 52, borderRadius: 'var(--r-md)', objectFit: 'cover' }}
            />
          )}

          <div style={{ minWidth: 0 }}>
            <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 3 }}>
              <strong style={{ fontSize: 'var(--fs-lg)' }}>{channel.name}</strong>
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
                      fontSize: 'var(--fs-xs)', color: 'var(--text-muted)',
                      fontVariantNumeric: 'tabular-nums',
                    }}
                  >
                    {clockTime(data.now.start)}–{clockTime(data.now.stop)}
                  </span>
                  <div
                    style={{
                      flex: 1, maxWidth: 280, height: 3, background: 'var(--border)',
                      borderRadius: 'var(--r-full)', overflow: 'hidden',
                    }}
                  >
                    <div style={{ width: `${pct}%`, height: '100%', background: 'var(--live)' }} />
                  </div>
                </div>
                {data.next && (
                  <div
                    style={{
                      marginTop: 5, fontSize: 'var(--fs-xs)', color: 'var(--text-faint)',
                      whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis',
                    }}
                  >
                    Next: {clockTime(data.next.start)} {data.next.title}
                  </div>
                )}
              </>
            ) : (
              <div style={{ fontSize: 'var(--fs-sm)', color: 'var(--text-faint)' }}>
                No guide data for this channel
              </div>
            )}
          </div>

          <div
            style={{
              fontSize: 'var(--fs-xl)', fontWeight: 700, color: 'var(--text-muted)',
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
            background: 'rgb(8 8 12 / 0.92)', backdropFilter: 'blur(18px)',
            border: '2px solid var(--accent)', borderRadius: 'var(--r-lg)',
            padding: 'var(--sp-4) var(--sp-6)', boxShadow: 'var(--shadow-4)',
            fontSize: 'var(--fs-3xl)', fontWeight: 900, letterSpacing: '0.12em',
            fontVariantNumeric: 'tabular-nums', pointerEvents: 'none',
          }}
        >
          {digits.padEnd(3, '–')}
        </motion.div>
      )}
    </AnimatePresence>
  );
}
