/**
 * The Gemini rail.
 *
 * Loaded on its own rather than with the other rails, because it is the one that can take
 * several seconds: `library.rails` is a database query and this is a model generation.
 * Folding it into that call would hold the whole home screen behind it, and a home screen
 * that waits on a network round trip is a worse trade than a rail that arrives late.
 *
 * It renders nothing at all until it has something — no heading, no skeleton, no empty
 * state. Every other failure mode here is quiet by design: no key set, nothing watched
 * yet, the model refused, none of its suggestions are in this library. Each of those is a
 * reason for the rail not to exist on this screen, and none of them is worth a row of
 * apology on somebody's home page. Settings is where it says what is wrong.
 */
import { useEffect, useState } from 'react';
import type { CatalogItem } from '@shared/ipc';
import { Rail } from '@/components/Rail';
import { invoke } from '@/ipc';
import { useProfile } from '@/state/profile';
import { useUi } from '@/state/ui';

interface Picks {
  items: CatalogItem[];
  reasons: Record<string, string>;
}

export function AiRail({
  onOpen, onPlay,
}: { onOpen: (i: CatalogItem) => void; onPlay: (i: CatalogItem) => void }) {
  const profileId = useProfile((s) => s.active?.id ?? 1);
  const catalogVersion = useUi((s) => s.catalogVersion);
  const [picks, setPicks] = useState<Picks | null>(null);

  useEffect(() => {
    let live = true;
    // Never surfaced. See the note above: every way this fails is a reason for the rail
    // not to be here, and the viewer did not ask for it on this screen.
    invoke('gemini.recommendations', { profileId })
      .then((p) => { if (live) setPicks({ items: p.items, reasons: p.reasons }); })
      .catch(() => { if (live) setPicks(null); });
    return () => { live = false; };
  }, [profileId, catalogVersion]);

  if (!picks || picks.items.length === 0) return null;

  return (
    <Rail
      rail={{
        id: 'gemini',
        kind: 'aiPicks',
        // Named for what it is. "Recommended for you" is what the local recommender's
        // rail already says, and two rails with one name is a screen that looks confused
        // about where its suggestions come from.
        title: 'Because of what you watch',
        items: picks.items,
        reasons: picks.reasons,
      }}
      onOpen={onOpen}
      onPlay={onPlay}
    />
  );
}
