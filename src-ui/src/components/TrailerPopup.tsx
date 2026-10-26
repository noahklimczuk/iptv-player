/**
 * The hover trailer, at a size worth watching.
 *
 * It used to play inside the card: the frame is 16:9 and a poster is 2:3, so the trailer
 * covered the middle band of a 150px-wide tile and the artwork filled the rest. That is
 * big enough to tell you a trailer exists and too small to watch, which is the worst of
 * both — it costs a video player either way.
 *
 * So it plays over the page instead, centred and large, while the card underneath keeps
 * its poster. Reported as "make it play in a bigger popup instead of the tiny one in the
 * poster preview".
 *
 * Deliberately *not* a modal: it opens on hover, so it must not trap focus, must not
 * need dismissing, and must not stop the viewer carrying on down the rail. It is a thing
 * being shown, not a thing being asked. Pointer events pass through it for the same
 * reason — moving the mouse off the card is what closes it, and a popup that swallowed
 * that movement would stay up until the pointer found a way around it.
 */
import { motion } from 'framer-motion';
import { useEffect } from 'react';
import { TrailerFrame } from './TrailerFrame';

/**
 * How much of the window the trailer takes.
 *
 * Capped in pixels as well as in viewport units: on a very wide screen a percentage
 * alone makes a 2,000px-wide trailer out of a 1080p source, which is upscaling for the
 * sake of it. 900px is comfortably bigger than the card and short of that.
 */
const MAX_WIDTH_PX = 900;

export function TrailerPopup({
  trailerKey,
  title,
  onClose,
}: {
  trailerKey: string;
  title: string;
  /** Called on Escape. Leaving the card closes it too, from the card's own handler. */
  onClose: () => void;
}) {
  useEffect(() => {
    // Escape closes it, because a thing covering the screen that cannot be dismissed by
    // the key everybody tries is a thing that feels stuck — even when moving the mouse
    // would also have done it.
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose]);

  return (
    <motion.div
      data-testid="trailer-popup"
      initial={{ opacity: 0, scale: 0.98 }}
      animate={{ opacity: 1, scale: 1 }}
      exit={{ opacity: 0, scale: 0.98 }}
      transition={{ duration: 0.16 }}
      style={{
        position: 'fixed', inset: 0, zIndex: 200,
        display: 'grid', placeItems: 'center',
        // Dimmed enough to read the trailer against, not so much that the page is gone:
        // the viewer is still browsing a rail, and this is a preview on top of it.
        background: 'rgba(0, 0, 0, 0.72)',
        // Hover-driven, so it must not eat the pointer. See the note above.
        pointerEvents: 'none',
      }}
    >
      <div
        style={{
          width: `min(86vw, ${MAX_WIDTH_PX}px)`,
          aspectRatio: '16 / 9',
          background: '#000',
          borderRadius: 'var(--r-lg)',
          overflow: 'hidden',
          boxShadow: 'var(--shadow-3)',
        }}
      >
        <TrailerFrame trailerKey={trailerKey} muted title={`Trailer for ${title}`} />
      </div>
      <div
        style={{
          position: 'absolute',
          bottom: 'max(var(--sp-6), 8vh)',
          color: 'var(--text-muted)',
          fontSize: 'var(--fs-sm)',
        }}
      >
        {title}
      </div>
    </motion.div>
  );
}
