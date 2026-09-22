/**
 * Skip Intro / Skip Recap / Skip Credits (README §9).
 *
 * Appears only while the playhead is inside a marker, sits clear of the transport bar,
 * and seeks to the end of the region. When the viewer has asked this show to always
 * skip, it performs the jump itself instead of waiting to be pressed — once per region,
 * so scrubbing back into an intro does not fight the viewer.
 */
import { AnimatePresence, motion } from 'framer-motion';
import { useEffect, useRef } from 'react';
import type { MarkerKind, SkipMarker } from '@shared/ipc';
import { Icon } from '@/components/Icon';

const LABELS: Record<MarkerKind, string> = {
  intro: 'Skip Intro',
  recap: 'Skip Recap',
  credits: 'Skip Credits',
};

export function SkipButton({
  marker, autoSkip, onSkip,
}: {
  marker: SkipMarker | null;
  /** The viewer has asked this show to skip this kind automatically. */
  autoSkip: boolean;
  onSkip: (marker: SkipMarker, automatic: boolean) => void;
}) {
  // Remember which regions we already auto-skipped, so seeking back in does not
  // immediately yank the viewer forward again.
  const autoSkipped = useRef(new Set<string>());

  useEffect(() => {
    if (!marker || !autoSkip) return;
    const key = `${marker.kind}:${marker.startSecs}`;
    if (autoSkipped.current.has(key)) return;
    autoSkipped.current.add(key);
    onSkip(marker, true);
  }, [marker, autoSkip, onSkip]);

  return (
    <AnimatePresence>
      {marker && (
        <motion.button
          key={`${marker.kind}-${marker.startSecs}`}
          initial={{ opacity: 0, y: 12, scale: 0.96 }}
          animate={{ opacity: 1, y: 0, scale: 1 }}
          exit={{ opacity: 0, y: 12, scale: 0.96 }}
          transition={{ duration: 0.18, ease: [0.16, 1, 0.3, 1] }}
          onClick={() => onSkip(marker, false)}
          aria-label={LABELS[marker.kind]}
          style={{
            display: 'inline-flex',
            alignItems: 'center',
            gap: 'var(--sp-2)',
            padding: '12px 22px',
            borderRadius: 'var(--r-md)',
            border: '2px solid var(--text)',
            background: 'color-mix(in srgb, var(--bg) 78%, transparent)',
            backdropFilter: 'blur(14px)',
            color: 'var(--text)',
            fontSize: 'var(--fs-md)',
            fontWeight: 700,
            cursor: 'pointer',
            boxShadow: 'var(--shadow-3)',
          }}
        >
          <Icon name="skip" size={18} filled />
          {LABELS[marker.kind]}
          {marker.source === 'learned' && (
            <span
              title="Learned from your earlier skips in this show"
              style={{
                fontSize: 'var(--fs-xs)',
                fontWeight: 600,
                color: 'var(--accent-2)',
                opacity: 0.9,
              }}
            >
              learned
            </span>
          )}
        </motion.button>
      )}
    </AnimatePresence>
  );
}
