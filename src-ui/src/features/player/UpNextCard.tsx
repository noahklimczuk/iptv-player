/**
 * "Up Next" autoplay card (README §9): when an episode reaches its credits, a corner
 * card counts down to the next episode over the tail of the current one, with
 * "Play now" and "Cancel".
 *
 * Cancelling is remembered for this episode, so dismissing it once does not mean
 * fighting it every few seconds until the credits end.
 */
import { AnimatePresence, motion } from 'framer-motion';
import { useEffect, useState } from 'react';
import type { Episode } from '@shared/ipc';
import { Button } from '@/components/Primitives';
import { runtime } from '@/lib/format';

export const COUNTDOWN_SECS = 10;

export function UpNextCard({
  episode, visible, onPlay, onCancel, autoplay,
}: {
  episode: Episode | null;
  visible: boolean;
  onPlay: () => void;
  onCancel: () => void;
  /** When off, the card still offers the next episode but never plays it by itself. */
  autoplay: boolean;
}) {
  const [remaining, setRemaining] = useState(COUNTDOWN_SECS);

  useEffect(() => {
    if (!visible || !autoplay) {
      setRemaining(COUNTDOWN_SECS);
      return;
    }
    setRemaining(COUNTDOWN_SECS);
    const id = setInterval(() => {
      setRemaining((r) => {
        if (r <= 1) {
          clearInterval(id);
          onPlay();
          return 0;
        }
        return r - 1;
      });
    }, 1000);
    return () => clearInterval(id);
  }, [visible, autoplay, onPlay]);

  const progress = ((COUNTDOWN_SECS - remaining) / COUNTDOWN_SECS) * 100;

  return (
    <AnimatePresence>
      {visible && episode && (
        <motion.aside
          initial={{ opacity: 0, x: 40 }}
          animate={{ opacity: 1, x: 0 }}
          exit={{ opacity: 0, x: 40 }}
          transition={{ duration: 0.24, ease: [0.16, 1, 0.3, 1] }}
          aria-label="Up next"
          style={{
            width: 380,
            borderRadius: 'var(--r-lg)',
            overflow: 'hidden',
            background: 'rgb(8 8 12 / 0.94)',
            backdropFilter: 'blur(18px)',
            border: '1px solid var(--border-strong)',
            boxShadow: 'var(--shadow-4)',
          }}
        >
          <div style={{ display: 'flex', gap: 'var(--sp-3)', padding: 'var(--sp-3)' }}>
            {episode.still && (
              <img
                src={episode.still}
                alt=""
                style={{
                  width: 132,
                  aspectRatio: '16/9',
                  objectFit: 'cover',
                  borderRadius: 'var(--r-sm)',
                  flexShrink: 0,
                }}
              />
            )}
            <div style={{ minWidth: 0, flex: 1 }}>
              <div
                style={{
                  fontSize: 'var(--fs-xs)',
                  fontWeight: 700,
                  letterSpacing: '0.07em',
                  textTransform: 'uppercase',
                  color: 'var(--text-faint)',
                  marginBottom: 3,
                }}
              >
                {autoplay ? `Up next in ${remaining}s` : 'Up next'}
              </div>
              <div
                style={{
                  fontWeight: 700,
                  fontSize: 'var(--fs-md)',
                  whiteSpace: 'nowrap',
                  overflow: 'hidden',
                  textOverflow: 'ellipsis',
                }}
              >
                {episode.title ?? `Episode ${episode.episode}`}
              </div>
              <div style={{ fontSize: 'var(--fs-xs)', color: 'var(--text-muted)' }}>
                S{episode.season} E{episode.episode}
                {episode.runtimeMins ? ` · ${runtime(episode.runtimeMins)}` : ''}
              </div>
            </div>
          </div>

          <div
            style={{
              display: 'flex',
              gap: 'var(--sp-2)',
              padding: '0 var(--sp-3) var(--sp-3)',
            }}
          >
            <Button variant="primary" size="sm" icon="play" iconFilled onClick={onPlay}>
              Play now
            </Button>
            <Button size="sm" onClick={onCancel}>Cancel</Button>
          </div>

          {autoplay && (
            <div
              style={{ height: 3, background: 'var(--border)' }}
              role="progressbar"
              aria-valuenow={Math.round(progress)}
              aria-valuemin={0}
              aria-valuemax={100}
            >
              <div
                style={{
                  height: '100%',
                  width: `${progress}%`,
                  background: 'var(--accent)',
                  transition: 'width 1s linear',
                }}
              />
            </div>
          )}
        </motion.aside>
      )}
    </AnimatePresence>
  );
}
