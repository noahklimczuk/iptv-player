/**
 * Drives Skip Intro and Up Next for whatever episode is playing (README §9).
 *
 * Holds no playback state of its own: it reads the mirrored player state, asks the
 * host once per episode for markers and the next episode, and derives everything else
 * from the playhead.
 */
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { PlaybackAids, PlayerState, SkipMarker } from '@shared/ipc';
import { invoke } from '@/ipc';

export interface EpisodeAids {
  aids: PlaybackAids | null;
  /** The marker under the playhead right now, if any. */
  activeMarker: SkipMarker | null;
  /** Whether the current marker should be skipped without being asked. */
  autoSkip: boolean;
  upNextVisible: boolean;
  skip: (marker: SkipMarker, automatic: boolean) => void;
  playNext: () => void;
  dismissUpNext: () => void;
}

export function useEpisodeAids(
  player: PlayerState | null,
  onPlayEpisode: (episodeId: number) => void,
  profileId = 1,
): EpisodeAids {
  const [aids, setAids] = useState<PlaybackAids | null>(null);
  const [dismissed, setDismissed] = useState<number | null>(null);

  const episodeId =
    player?.itemKind === 'episode' && player.itemId != null ? player.itemId : null;
  const duration = player?.durationSecs ?? 0;
  const position = player?.positionSecs ?? 0;

  // Fetch once per episode. Duration is only read on the first load that has one, so
  // a late-arriving duration does not cause a refetch loop.
  const fetchedFor = useRef<number | null>(null);
  useEffect(() => {
    if (episodeId == null || duration <= 0) {
      if (episodeId == null) {
        setAids(null);
        fetchedFor.current = null;
      }
      return;
    }
    if (fetchedFor.current === episodeId) return;
    fetchedFor.current = episodeId;
    setDismissed(null);
    void invoke('library.playbackAids', {
      profileId,
      episodeId,
      durationSecs: duration,
    }).then(setAids);
  }, [episodeId, duration, profileId]);

  const activeMarker = useMemo(() => {
    if (!aids) return null;
    // Matches aurora_core::markers::SkipMarker::contains — the button releases just
    // before the region ends so it never covers content.
    return (
      aids.markers.find((m) => position >= m.startSecs && position < m.endSecs - 0.5) ??
      null
    );
  }, [aids, position]);

  const autoSkip =
    !!activeMarker &&
    !!aids &&
    ((activeMarker.kind === 'intro' && aids.prefs.alwaysSkipIntro) ||
      (activeMarker.kind === 'recap' && aids.prefs.alwaysSkipRecap));

  const skip = useCallback(
    (marker: SkipMarker, automatic: boolean) => {
      void invoke('player.seek', { positionSecs: marker.endSecs });
      // A press is a statement about where this show's intro really is, so remember
      // it; an automatic jump is just us replaying what we already knew.
      if (!automatic && episodeId != null) {
        void invoke('library.recordSkip', {
          episodeId,
          kind: marker.kind,
          startSecs: marker.startSecs,
          endSecs: marker.endSecs,
        });
      }
    },
    [episodeId],
  );

  const upNextVisible =
    !!aids?.nextEpisode &&
    aids.upNextAtSecs != null &&
    duration > 0 &&
    position >= aids.upNextAtSecs &&
    dismissed !== episodeId;

  const playNext = useCallback(() => {
    const next = aids?.nextEpisode;
    if (!next) return;
    onPlayEpisode(next.id);
  }, [aids, onPlayEpisode]);

  const dismissUpNext = useCallback(() => setDismissed(episodeId), [episodeId]);

  return { aids, activeMarker, autoSkip, upNextVisible, skip, playNext, dismissUpNext };
}
