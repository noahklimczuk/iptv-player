/**
 * Up Next (docs/PLAYER.md C23): at the credits, a card counts down to the next episode.
 * Play now starts it at once; Cancel stops the countdown for this episode.
 *
 * The countdown runs against a deadline rather than by counting ticks, so a slow or
 * throttled timer cannot stretch it, and it plays the next episode exactly once.
 */
import './player.css';
import { useEffect, useRef, useState } from 'react';
import type { Episode } from '@shared/ipc';
import { Icon } from '@/components/Icon';
import { runtime } from '@/lib/format';

export const COUNTDOWN_SECS = 10;

export function UpNextCard({
  episode, visible, onPlay, onCancel, autoplay,
}: {
  episode: Episode | null;
  visible: boolean;
  onPlay: () => void;
  onCancel: () => void;
  /** Off: the card offers the next episode but never plays it by itself. */
  autoplay: boolean;
}) {
  const shown = visible && episode != null;
  const [left, setLeft] = useState(COUNTDOWN_SECS);
  const play = useRef(onPlay);
  play.current = onPlay;

  useEffect(() => {
    setLeft(COUNTDOWN_SECS);
    if (!shown || !autoplay) return;
    const deadline = Date.now() + COUNTDOWN_SECS * 1000;
    let done = false;
    const tick = () => {
      const remaining = Math.max(0, Math.ceil((deadline - Date.now()) / 1000));
      setLeft(remaining);
      if (remaining === 0 && !done) {
        done = true;
        window.clearInterval(id);
        play.current();
      }
    };
    const id = window.setInterval(tick, 250);
    return () => { done = true; window.clearInterval(id); };
  }, [shown, autoplay, episode?.id]);

  if (!shown) return null;
  const progress = ((COUNTDOWN_SECS - left) / COUNTDOWN_SECS) * 100;

  return (
    <aside className="pl-upnext" aria-label="Up next">
      <div className="pl-upnext-body">
        {episode.still && <img src={episode.still} alt="" className="pl-upnext-still" />}
        <div className="pl-upnext-text">
          <div className="pl-card-kicker">{autoplay ? `Up next in ${left}s` : 'Up next'}</div>
          <div className="pl-upnext-title">{episode.title ?? `Episode ${episode.episode}`}</div>
          <div className="pl-upnext-meta">
            S{episode.season} E{episode.episode}
            {episode.runtimeMins ? ` · ${runtime(episode.runtimeMins)}` : ''}
          </div>
        </div>
      </div>
      <div className="pl-upnext-actions">
        <button type="button" className="pl-upnext-play" onClick={onPlay}>
          <Icon name="play" size={14} filled /> Play now
        </button>
        <button type="button" className="pl-upnext-cancel" onClick={onCancel}>Cancel</button>
      </div>
      {autoplay && (
        <div
          className="pl-upnext-bar"
          role="progressbar"
          aria-label="Countdown"
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={Math.round(progress)}
        >
          <div style={{ width: `${progress}%` }} />
        </div>
      )}
    </aside>
  );
}
