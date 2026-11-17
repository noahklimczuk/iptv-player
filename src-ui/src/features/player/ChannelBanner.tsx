/**
 * The channel banner and number entry (docs/PLAYER.md C19, C20).
 *
 * The banner appears on every zap: the channel's number, logo and name, what is on now
 * with its time range and how far through it is, and what is on next. Over the player it
 * sits at the top, clear of the dock; elsewhere at the foot of the page.
 */
import './player.css';
import type { Channel } from '@shared/ipc';
import { ChannelTile } from '@/components/Primitives';
import { useCommand } from '@/hooks/useCommand';
import { useClockTime } from '@/hooks/useClockTime';

export function ChannelBanner({
  channel, liftForOsd = false,
}: {
  channel: Channel | null;
  /** The player is open: sit at the top, clear of its dock. */
  liftForOsd?: boolean;
}) {
  const clock = useClockTime();
  const { data } = useCommand('epg.nowNext', { channelId: channel?.id ?? 0 }, [channel?.id], channel != null);
  if (!channel) return null;

  const nowSecs = Math.floor(Date.now() / 1000);
  const on = data?.now ?? null;
  const next = data?.next ?? null;
  const through = on
    ? Math.min(100, Math.max(0, ((nowSecs - on.start) / Math.max(1, on.stop - on.start)) * 100))
    : 0;

  return (
    <div
      data-testid="channel-banner"
      className={liftForOsd ? 'pl-banner pl-banner-top' : 'pl-banner pl-banner-bottom'}
      role="status"
      aria-live="polite"
      aria-label={`${channel.number ?? ''} ${channel.name}${on ? `, now ${on.title}` : ''}`.trim()}
    >
      <ChannelTile name={channel.name} number={channel.number} logo={channel.logo} size={52} />
      <div className="dl-display pl-banner-number">{channel.number}</div>
      <div className="pl-banner-main">
        <div className="pl-banner-name">
          <strong>{channel.name}</strong>
          {channel.quality && <span className="pl-tag pl-tag-quiet">{channel.quality}</span>}
        </div>
        {on ? (
          <>
            <div className="pl-banner-now">{on.title}</div>
            <div className="pl-banner-time">
              <span>{clock(on.start)}–{clock(on.stop)}</span>
              <div
                className="pl-progress pl-banner-progress"
                role="progressbar"
                aria-label="Programme progress"
                aria-valuemin={0}
                aria-valuemax={100}
                aria-valuenow={Math.round(through)}
              >
                <div style={{ width: `${through}%` }} />
              </div>
            </div>
            {next && <div className="pl-banner-next">Next: {clock(next.start)} {next.title}</div>}
          </>
        ) : (
          <div className="pl-banner-next">No guide data for this channel</div>
        )}
      </div>
      <div className="pl-banner-clock">{clock(nowSecs)}</div>
    </div>
  );
}

/** "2 _ _" while a channel number is being typed (C20). */
export function DigitEntry({ digits }: { digits: string }) {
  if (!digits) return null;
  return (
    <div className="pl-digits" role="status" aria-live="polite" aria-label={`Channel ${digits}`}>
      {digits.padEnd(3, '–')}
    </div>
  );
}
