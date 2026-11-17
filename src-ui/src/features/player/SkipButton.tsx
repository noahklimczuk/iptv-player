/**
 * Skip Intro / Recap / Credits (docs/PLAYER.md C22).
 *
 * Shown only while the playhead is inside a marked stretch, and pressing it seeks to the
 * stretch's end. A marker the library learned from the viewer's own skips says so. When
 * the viewer has asked this show to always skip, the jump happens by itself, once per
 * stretch, so seeking back into an intro does not throw them forward again. The parent
 * keys this on the episode, so that memory is per episode.
 */
import './player.css';
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
  autoSkip: boolean;
  onSkip: (marker: SkipMarker, automatic: boolean) => void;
}) {
  const skipped = useRef(new Set<string>());

  useEffect(() => {
    if (!marker || !autoSkip) return;
    const key = `${marker.kind}:${marker.startSecs}`;
    if (skipped.current.has(key)) return;
    skipped.current.add(key);
    onSkip(marker, true);
  }, [marker, autoSkip, onSkip]);

  if (!marker) return null;
  const learned = marker.source === 'learned';
  return (
    <button
      key={`${marker.kind}-${marker.startSecs}`}
      type="button"
      className="pl-skip"
      aria-label={LABELS[marker.kind]}
      title={learned ? 'Learned from your skips in this show' : LABELS[marker.kind]}
      onClick={() => onSkip(marker, false)}
    >
      <Icon name="skip" size={18} filled />
      <span>{LABELS[marker.kind]}</span>
      {learned && <span className="pl-skip-learned">Learned from your skips</span>}
    </button>
  );
}
