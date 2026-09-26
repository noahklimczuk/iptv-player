/**
 * Hero billboard (README §8.1): full-bleed backdrop, metadata row, truncated synopsis,
 * Play / My List / More Info. After ~2s it crossfades into a muted "trailer" preview,
 * with a mute toggle and rotation through several picks. Respects Reduce Motion.
 */
import { AnimatePresence, motion } from 'framer-motion';
import { useEffect, useState } from 'react';
import type { CatalogItem } from '@shared/ipc';
import { runtime } from '@/lib/format';
import { useUi } from '@/state/ui';
import { Badge, Button, IconButton } from './Primitives';

const TRAILER_DELAY_MS = 2000;
const ROTATE_MS = 12000;

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

  useEffect(() => {
    setTrailer(false);
    if (!animations || !hoverPreviews) return;
    const t = window.setTimeout(() => setTrailer(true), TRAILER_DELAY_MS);
    return () => window.clearTimeout(t);
  }, [index, animations, hoverPreviews]);

  useEffect(() => {
    if (paused || items.length < 2) return;
    const t = window.setTimeout(() => setIndex((i) => (i + 1) % items.length), ROTATE_MS);
    return () => window.clearTimeout(t);
  }, [index, paused, items.length]);

  if (!item) return null;

  return (
    <section
      aria-label="Featured"
      onMouseEnter={() => setPaused(true)}
      onMouseLeave={() => setPaused(false)}
      style={{ position: 'relative', height: '62vh', minHeight: 420, marginBottom: 'var(--sp-5)' }}
    >
      <AnimatePresence mode="wait">
        <motion.div
          key={item.id}
          initial={{ opacity: 0, scale: 1.04 }}
          animate={{ opacity: 1, scale: trailer ? 1.06 : 1 }}
          exit={{ opacity: 0 }}
          transition={{
            opacity: { duration: animations ? 0.5 : 0 },
            scale: { duration: animations ? 8 : 0, ease: 'linear' },
          }}
          style={{ position: 'absolute', inset: 0 }}
        >
          {/* A backdrop if there is one, the poster blown out behind glass if not.
              Backdrops come from TMDB enrichment, so on a library without an API key
              there are none at all — and the most prominent thing on the home screen
              was a black rectangle four hundred pixels tall. A poster is the wrong
              shape for this, hence the crop and the blur: what is wanted is colour
              and motion behind the title, not a legible second copy of the artwork. */}
          {item.backdrop ? (
            <img
              src={item.backdrop} alt=""
              style={{ width: '100%', height: '100%', objectFit: 'cover' }}
            />
          ) : item.poster ? (
            <img
              src={item.poster} alt=""
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

      <div style={{ position: 'absolute', inset: 0, background: 'var(--scrim)' }} />
      <div style={{ position: 'absolute', inset: 0, background: 'var(--scrim-side)' }} />

      <div
        style={{
          position: 'absolute', left: 'var(--sp-6)', bottom: 'var(--sp-6)',
          maxWidth: 'min(560px, 52%)',
        }}
      >
        {trailer && (
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
          <span>
            {item.kind === 'series'
              ? `${item.seasons.length} season${item.seasons.length === 1 ? '' : 's'}`
              : runtime(item.runtimeMins)}
          </span>
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
        {trailer && (
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
