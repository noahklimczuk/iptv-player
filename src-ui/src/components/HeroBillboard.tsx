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
import { useMarks } from '@/state/marks';

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

/** Buttons on artwork: one solid paper button, the rest frosted. */
const ON_ART_SOLID: React.CSSProperties = {
  background: 'var(--on-art)', color: 'var(--text-on-light)',
};
const ON_ART_GLASS: React.CSSProperties = {
  background: 'color-mix(in srgb, var(--on-art) 20%, transparent)',
  color: 'var(--on-art)', boxShadow: 'none',
  backdropFilter: 'blur(12px)',
};

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
  const myList = useMarks((s) => s.myList);
  const toggleMyList = useMarks((s) => s.toggleMyList);

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
    // `animations` stops it entirely, which is what README §12 asks for and what the
    // reduce-motion setting means: a billboard that re-renders itself every twelve
    // seconds is motion, whether or not it is animated on the way.
    if (paused || !animations || items.length < 2) return;
    const t = window.setTimeout(
      () => setIndex((i) => (i + 1) % items.length),
      trailer ? ROTATE_WITH_TRAILER_MS : ROTATE_MS,
    );
    return () => window.clearTimeout(t);
  }, [index, paused, animations, items.length, trailer]);

  if (!item) return null;

  return (
    <section
      aria-label="Featured"
      onMouseEnter={() => setPaused(true)}
      onMouseLeave={() => setPaused(false)}
      /*
       * And for the keyboard, which had nothing.
       *
       * Pausing was hover-only, and this billboard carries three large focusable
       * buttons. So somebody who tabbed to **Play** and stopped to read the synopsis got
       * twelve seconds — thirty-two if a trailer had started — before `setIndex`
       * advanced: the focused element is not remounted, so focus stays put and the label
       * changes under it, and Enter plays a film they did not choose. WCAG 2.2.2 asks
       * for a way to pause anything moving for more than five seconds, and the dot
       * indicators let a viewer jump but not stop (R-151).
       *
       * `onFocus`/`onBlur` bubble from the buttons inside, which is the same one-line
       * omission as the episode rows in `DetailModal` and `SelectRow`'s hover.
       */
      onFocus={() => setPaused(true)}
      onBlur={() => setPaused(false)}
      style={{
        // Daylight: a bento tile rather than a full-bleed band. It fills the grid cell
        // Home gives it, in the accent blue whenever there is no artwork to show.
        position: 'relative', flex: 1, minHeight: 320, width: '100%',
        borderRadius: 'var(--r-xl)', background: 'var(--art-1)',
        color: 'var(--on-art)',
        // The trailer frame is scaled up to hide its letterboxing; this is what keeps
        // the overscan inside the billboard.
        overflow: 'hidden',
      }}
    >
      {/* A crossfade, not a handover. `mode="wait"` held the next backdrop until the
          previous one had finished leaving, and both are `position: absolute` — so every
          rotation went through half a second of empty billboard with the title text
          floating over nothing. Overlapping them is what a dissolve is. */}
      <AnimatePresence>
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
            // Nothing at all to show: the tile's own colour, with Daylight's big soft
            // circle so it reads as designed rather than as missing.
            <div
              style={{
                width: '100%', height: '100%',
                background:
                  'radial-gradient(circle at 78% 18%, color-mix(in srgb, var(--on-art) 14%, transparent) 0 46%, transparent 46.2%)',
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

      {(backdrop || poster || trailer) && (
        <>
          <div style={{ position: 'absolute', inset: 0, background: 'var(--scrim)' }} />
          <div style={{ position: 'absolute', inset: 0, background: 'var(--scrim-side)' }} />
        </>
      )}

      <div
        style={{
          position: 'absolute', left: 32, right: 32, top: 28, bottom: 32,
          maxWidth: 'min(640px, 86%)',
          display: 'flex', flexDirection: 'column', justifyContent: 'flex-end',
          alignItems: 'flex-start', overflow: 'hidden',
        }}
      >
        <span
          className="dl-chip"
          style={{
            background: 'var(--on-art)', color: 'var(--text-on-light)',
            marginBottom: 14,
          }}
        >
          {item.kind === 'series' ? 'Featured series' : 'Featured film'}
        </span>
        {/* Only with a key, so the claim is never made about a title that has no
            trailer. `trailer` cannot be set without one, and saying so here keeps the
            two from drifting apart. */}
        {trailer && trailerKey && (
          <div
            style={{
              display: 'flex', alignItems: 'center', gap: 6, marginBottom: 'var(--sp-2)',
              fontSize: 'var(--fs-xs)', color: 'var(--on-art)', fontWeight: 800,
              letterSpacing: '0.06em',
            }}
          >
            <span
              style={{ width: 6, height: 6, borderRadius: '50%', background: 'var(--live)' }}
            />
            NOW PLAYING TRAILER
          </div>
        )}

        <h1
          className="dl-display"
          style={{
            margin: '0 0 10px', fontSize: 'clamp(34px, 3.8vw, 60px)',
            lineHeight: 0.98, color: 'var(--on-art)',
            display: '-webkit-box', WebkitLineClamp: 2, WebkitBoxOrient: 'vertical',
            overflow: 'hidden',
          }}
        >
          {item.title}
        </h1>

        <div
          style={{
            display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap',
            marginBottom: 'var(--sp-3)', fontSize: 'var(--fs-sm)', fontWeight: 700,
            color: 'color-mix(in srgb, var(--on-art) 88%, transparent)',
          }}
        >
          {/* "% match" was here too, and was mock-only in the same way -- `match` was
              assigned in `fixtures.ts` and by nothing on the host, so this branch never
              ran outside a browser (R-99). */}
          {item.year && <span>{item.year}</span>}
          {item.certification && <span>· {item.certification}</span>}
          {/* See `CatalogCard`: unknown until the listing is fetched, and "0
              seasons" is worse than saying nothing. */}
          {item.kind === 'series' && item.seasons.length > 0 && (
            <span>
              {item.seasons.length} season{item.seasons.length === 1 ? '' : 's'}
            </span>
          )}
          {item.kind !== 'series' && <span>· {runtime(item.runtimeMins)}</span>}
          {item.quality === '4K' && <Badge tone="accent">4K</Badge>}
        </div>

        <p
          style={{
            margin: '0 0 20px', fontSize: 'var(--fs-md)', maxWidth: 520,
            color: 'color-mix(in srgb, var(--on-art) 85%, transparent)',
            lineHeight: 1.5, display: '-webkit-box', WebkitLineClamp: 2,
            WebkitBoxOrient: 'vertical', overflow: 'hidden',
          }}
        >
          {item.overview}
        </p>

        <div style={{ display: 'flex', gap: 'var(--sp-3)', alignItems: 'center' }}>
          <Button
            variant="primary" size="lg" icon="play" iconFilled onClick={() => onPlay(item)}
            style={ON_ART_SOLID}
          >
            Play
          </Button>
          {/* Was a button with no handler at all, on the most prominent surface in the
              app. `onMyList` is read here rather than through the hook because `item`
              comes from an array and the hooks above it must not be conditional. */}
          <Button
            variant="secondary"
            size="lg"
            icon={myList.has(`${item.kind === 'series' ? 'series' : 'movie'}:${item.id}`) ? 'check' : 'plus'}
            aria-pressed={myList.has(`${item.kind === 'series' ? 'series' : 'movie'}:${item.id}`)}
            onClick={() => toggleMyList(item)}
            style={ON_ART_GLASS}
          >
            My List
          </Button>
          <Button
            variant="secondary" size="lg" icon="info" onClick={() => onOpen(item)}
            style={ON_ART_GLASS}
          >
            More Info
          </Button>
        </div>
      </div>

      <div
        style={{
          position: 'absolute', right: 28, top: 28,
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
                width: i === index ? 22 : 8, height: 8, borderRadius: 'var(--r-full)',
                border: 'none', cursor: 'pointer', padding: 0,
                background: i === index
                  ? 'var(--on-art)'
                  : 'color-mix(in srgb, var(--on-art) 40%, transparent)',
                transition: 'width var(--t-base) var(--ease)',
              }}
            />
          ))}
        </div>
      </div>
    </section>
  );
}
