/**
 * Hero billboard (README §8.1): full-bleed backdrop, metadata row, truncated synopsis,
 * Play / My List / More Info. After ~2s it crossfades into the muted trailer, with a
 * mute toggle and rotation through several picks. Respects Reduce Motion.
 *
 * The trailer used to be imaginary: this printed "NOW PLAYING TRAILER", zoomed the
 * backdrop over eight seconds, and offered a speaker button wired to a state variable
 * that no audio existed to obey. It now plays the thing TMDB lists, when TMDB lists one
 * — and says nothing when it does not, which is most titles on a library whose metadata
 * sweep has not run.
 */
import { AnimatePresence, motion } from 'framer-motion';
import { useEffect, useState } from 'react';
import type { CatalogItem } from '@shared/ipc';
import { runtime } from '@/lib/format';
import { useUi } from '@/state/ui';
import { fallBackToRemote, useAssetSrc } from '@/hooks/useAssetSrc';

import { Badge, Button, IconButton } from './Primitives';
import { TrailerFrame } from './TrailerFrame';

const TRAILER_DELAY_MS = 2000;
const ROTATE_MS = 12000;

/**
 * How long a title stays up once its trailer is playing.
 *
 * Longer than the 12s a still backdrop gets: cutting away four seconds into a trailer is
 * worse than not starting one. Still bounded, because the billboard is a rotation and
 * not a cinema.
 */
const ROTATE_WITH_TRAILER_MS = 32000;

export function HeroBillboard({
  items, onOpen, onPlay,
}: {
  items: CatalogItem[];
  onOpen: (i: CatalogItem) => void;
  onPlay: (i: CatalogItem) => void;
}) {
  const [index, setIndex] = useState(0);
  const [trailer, setTrailer] = useState(false);
  const [muted, setMuted] = useState(true);
  const [paused, setPaused] = useState(false);
  const animations = useUi((s) => s.animations);
  const hoverPreviews = useUi((s) => s.hoverPreviews);

  const item = items[index];
  // Asked for unguarded, and before the `!item` return below: hooks cannot be called
  // conditionally, and `items` is empty on the first paint of a cold library.
  const backdrop = useAssetSrc(item?.backdrop);
  const poster = useAssetSrc(item?.poster);

  // Only when there is something to play. `trailer` used to mean "pretend", so it could
  // be turned on for anything; it now gates a real frame and a real claim.
  const trailerKey = item?.trailerKey ?? null;

  useEffect(() => {
    setTrailer(false);
    // Muted autoplay is the only autoplay an engine allows, so each title starts silent
    // however the last one was left. Keeping an unmute across a rotation would mean a
    // home screen that suddenly makes noise about a film nobody asked about.
    setMuted(true);
    if (!animations || !hoverPreviews || !trailerKey) return;
    const t = window.setTimeout(() => setTrailer(true), TRAILER_DELAY_MS);
    return () => window.clearTimeout(t);
  }, [index, animations, hoverPreviews, trailerKey]);

  useEffect(() => {
    if (paused || items.length < 2) return;
    const t = window.setTimeout(
      () => setIndex((i) => (i + 1) % items.length),
      trailer ? ROTATE_WITH_TRAILER_MS : ROTATE_MS,
    );
    return () => window.clearTimeout(t);
  }, [index, paused, items.length, trailer]);

  if (!item) return null;

  return (
    <section
      aria-label="Featured"
      onMouseEnter={() => setPaused(true)}
      onMouseLeave={() => setPaused(false)}
      style={{
        position: 'relative', height: '62vh', minHeight: 420,
        marginBottom: 'var(--sp-5)',
        // The trailer frame is scaled up to hide its letterboxing; this is what keeps
        // the overscan inside the billboard.
        overflow: 'hidden',
      }}
    >
      <AnimatePresence mode="wait">
        <motion.div
          key={item.id}
          initial={{ opacity: 0, scale: 1 }}
          // A slow drift on the still backdrop, not a response to the trailer: the
          // trailer covers this element, so a zoom that only ran while one was playing
          // was a zoom nobody could see. It is what keeps the hero from looking like a
          // screenshot while a title waits its turn.
          animate={{ opacity: 1, scale: animations ? 1.06 : 1 }}
          exit={{ opacity: 0 }}
          transition={{
            opacity: { duration: animations ? 0.5 : 0 },
            scale: { duration: animations ? 18 : 0, ease: 'linear' },
          }}
          style={{ position: 'absolute', inset: 0 }}
        >
          {/* A backdrop if there is one, the poster blown out behind glass if not.
              Backdrops come from TMDB enrichment, so on a library without an API key
              there are none at all — and the most prominent thing on the home screen
              was a black rectangle four hundred pixels tall. A poster is the wrong
              shape for this, hence the crop and the blur: what is wanted is colour
              and motion behind the title, not a legible second copy of the artwork. */}
          {backdrop ? (
            <img
              src={backdrop} alt=""
              onError={(e) => fallBackToRemote(e, item.backdrop)}
              style={{ width: '100%', height: '100%', objectFit: 'cover' }}
            />
          ) : poster ? (
            <img
              src={poster} alt=""
              onError={(e) => fallBackToRemote(e, item.poster)}
              style={{
                width: '100%', height: '100%', objectFit: 'cover',
                objectPosition: 'center 22%',
                filter: 'blur(28px) saturate(1.25) brightness(0.72)',
                transform: 'scale(1.15)',
              }}
            />
          ) : (
            // Nothing at all to show. A flat panel rather than a hole: the layout
            // below it assumes something is here.
            <div
              style={{
                width: '100%', height: '100%',
                background:
                  'radial-gradient(120% 90% at 20% 0%, var(--surface) 0%, var(--bg) 70%)',
              }}
            />
          )}
        </motion.div>
      </AnimatePresence>

      {/* Over the backdrop rather than instead of it: the frame is 16:9 and the
          billboard is not, so the backdrop fills what the video cannot and is what
          remains if the embed never loads. Scaled past the edges to keep YouTube's own
          letterboxing off screen — a black band across the hero reads as a layout fault,
          and `overflow: hidden` on the section is what stops the overscan showing. */}
      {trailer && trailerKey && (
        <div
          aria-hidden={false}
          style={{
            position: 'absolute', inset: 0, overflow: 'hidden',
            // Behind the scrims and the text below, in front of the backdrop.
            zIndex: 0,
          }}
        >
          <div
            style={{
              position: 'absolute', top: '50%', left: '50%',
              width: '100%', height: '100%',
              transform: 'translate(-50%, -50%) scale(1.35)',
            }}
          >
            <TrailerFrame
              trailerKey={trailerKey}
              muted={muted}
              title={`Trailer for ${item.title}`}
            />
          </div>
        </div>
      )}

      <div style={{ position: 'absolute', inset: 0, background: 'var(--scrim)' }} />
      <div style={{ position: 'absolute', inset: 0, background: 'var(--scrim-side)' }} />

      <div
        style={{
          position: 'absolute', left: 'var(--sp-6)', bottom: 'var(--sp-6)',
          maxWidth: 'min(560px, 52%)',
        }}
      >
        {/* Only with a key, so the claim is never made about a title that has no
            trailer. `trailer` cannot be set without one, and saying so here keeps the
            two from drifting apart. */}
        {trailer && trailerKey && (
          <div
            style={{
              display: 'flex', alignItems: 'center', gap: 6, marginBottom: 'var(--sp-2)',
              fontSize: 'var(--fs-xs)', color: 'var(--accent-2)', fontWeight: 700,
              letterSpacing: '0.06em',
            }}
          >
            <span
              style={{ width: 6, height: 6, borderRadius: '50%', background: 'var(--accent-2)' }}
            />
            NOW PLAYING TRAILER
          </div>
        )}

        <h1
          className="text-shadow-hero"
          style={{
            margin: '0 0 var(--sp-3)', fontSize: 'var(--fs-3xl)', fontWeight: 900,
            letterSpacing: '-0.03em', lineHeight: 1.05,
          }}
        >
          {item.title}
        </h1>

        <div
          style={{
            display: 'flex', alignItems: 'center', gap: 'var(--sp-3)', flexWrap: 'wrap',
            marginBottom: 'var(--sp-3)', fontSize: 'var(--fs-sm)',
          }}
        >
          {item.match !== undefined && (
            <span style={{ color: 'var(--success)', fontWeight: 800 }}>{item.match}% match</span>
          )}
          {item.year && <span>{item.year}</span>}
          {item.certification && <Badge tone="outline">{item.certification}</Badge>}
          {/* See `CatalogCard`: unknown until the listing is fetched, and "0
              seasons" is worse than saying nothing. */}
          {item.kind === 'series' && item.seasons.length > 0 && (
            <span>
              {item.seasons.length} season{item.seasons.length === 1 ? '' : 's'}
            </span>
          )}
          {item.kind !== 'series' && <span>{runtime(item.runtimeMins)}</span>}
          {item.quality === '4K' && <Badge tone="accent">4K</Badge>}
        </div>

        <p
          className="text-shadow-hero"
          style={{
            margin: '0 0 var(--sp-5)', color: 'var(--text)', fontSize: 'var(--fs-md)',
            lineHeight: 1.55, display: '-webkit-box', WebkitLineClamp: 2,
            WebkitBoxOrient: 'vertical', overflow: 'hidden',
          }}
        >
          {item.overview}
        </p>

        <div style={{ display: 'flex', gap: 'var(--sp-3)', alignItems: 'center' }}>
          <Button variant="primary" size="lg" icon="play" iconFilled onClick={() => onPlay(item)}>
            Play
          </Button>
          <Button variant="secondary" size="lg" icon="plus">My List</Button>
          <Button variant="secondary" size="lg" icon="info" onClick={() => onOpen(item)}>
            More Info
          </Button>
        </div>
      </div>

      <div
        style={{
          position: 'absolute', right: 'var(--sp-6)', bottom: 'var(--sp-6)',
          display: 'flex', alignItems: 'center', gap: 'var(--sp-3)',
        }}
      >
        {trailer && trailerKey && (
          <IconButton
            icon={muted ? 'volumeOff' : 'volume'}
            label={muted ? 'Unmute trailer' : 'Mute trailer'}
            onClick={() => setMuted((m) => !m)}
          />
        )}
        <div style={{ display: 'flex', gap: 5 }}>
          {items.map((it, i) => (
            <button
              key={it.id}
              aria-label={`Show ${it.title}`}
              onClick={() => setIndex(i)}
              style={{
                width: i === index ? 22 : 7, height: 3, borderRadius: 'var(--r-full)',
                border: 'none', cursor: 'pointer', padding: 0,
                background: i === index ? 'var(--text)' : 'var(--text-faint)',
                transition: 'width var(--t-base) var(--ease)',
              }}
            />
          ))}
        </div>
      </div>
    </section>
  );
}
