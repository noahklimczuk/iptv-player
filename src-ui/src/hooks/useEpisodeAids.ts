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
import { report } from '@/lib/errors';
import { activeProfileId } from '@/state/profile';

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
  profileId: number = activeProfileId(),
): EpisodeAids {
  const [aids, setAids] = useState<PlaybackAids | null>(null);
  const [dismissed, setDismissed] = useState<number | null>(null);

  const episodeId =
    player?.itemKind === 'episode' && player.itemId != null ? player.itemId : null;
  const duration = player?.durationSecs ?? 0;
  const position = player?.positionSecs ?? 0;

  // Fetch as soon as there is an episode, with whatever duration the player has — which
  // is routinely none.
  //
  // This used to wait for `duration > 0`, and that is the other half of the bug the
  // `upNextVisible` comment below describes. A provider's VOD stream often reports no
  // duration, sometimes never; the host was taught to fall back to the episode's stored
  // runtime for exactly that case, but it was never asked, so `aids` stayed null for the
  // whole episode. With no aids there is no `upNextAtSecs` *and* no markers and no next
  // episode either — so Skip Intro, Skip Credits and autoplay were all dead together on
  // any stream without a duration, however much evidence the library held. Fixing the
  // guard downstream could not help while the request was never made.
  //
  // Asking with `durationSecs: 0` is the honest thing: it says "the player does not
  // know", which is a question the host can answer.
  //
  // Refetched once if a real duration turns up later, because a conventional credits
  // marker is placed off the end of the file and the player's own number is the better
  // one. Tracked as a pair so that is a single extra call and not a loop: the second
  // fetch is only ever from "asked without a duration" to "asked with one".
  const fetchedFor = useRef<{ episodeId: number; withDuration: boolean } | null>(null);
  useEffect(() => {
    if (episodeId == null) {
      setAids(null);
      fetchedFor.current = null;
      return;
    }
    const done = fetchedFor.current;
    if (done?.episodeId === episodeId && (done.withDuration || duration <= 0)) return;
    // Only when the episode itself changes: the refetch must not un-dismiss a card the
    // viewer has already waved away.
    if (done?.episodeId !== episodeId) setDismissed(null);
    fetchedFor.current = { episodeId, withDuration: duration > 0 };
    const args = { profileId, episodeId, durationSecs: duration };
    // Chapters first, then ask what there is.
    //
    // `library.playbackAids` only reports markers that are already stored, and
    // `library.syncChapters` is the only thing that stores the ones derived from the
    // file's own chapter list. It was implemented on the host, registered, and called
    // from nowhere — so `skip_markers` was empty on every real library and Skip Intro
    // could never appear, however many chapters the file had. Same shape as the
    // `progress.save` bug above it.
    //
    // Sequenced rather than run alongside: the point is that the write lands before the
    // read. A failure is not fatal — a file with no chapters is the normal case, and
    // then there is simply nothing to skip — so the aids are fetched either way.
    void invoke('library.syncChapters', args)
      .catch(() => {})
      .then(() =>
        invoke('library.playbackAids', args)
          .then(setAids)
          // Not fatal: these drive Skip Intro and Up Next, and a show without them is
          // simply a show you scrub yourself.
          .catch(report('Could not load this episode\u2019s skip markers')));
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

  /**
   * Whether to jump without being asked.
   *
   * `source !== 'convention'` is the guard that matters: a conventional marker is placed
   * by the clock rather than by evidence, so it is a button somebody may press and never
   * something that fires on its own. Auto-skipping a guess would mean a wrong runtime
   * silently cutting the last minute of an episode — which is how you lose the final
   * scene of a finale.
   */
  const autoSkip =
    !!activeMarker &&
    !!aids &&
    activeMarker.source !== 'convention' &&
    ((activeMarker.kind === 'intro' && aids.prefs.alwaysSkipIntro) ||
      (activeMarker.kind === 'recap' && aids.prefs.alwaysSkipRecap));

  const skip = useCallback(
    (marker: SkipMarker, automatic: boolean) => {
      invoke('player.seek', { positionSecs: marker.endSecs })
        .catch(report('Could not skip'));
      // A press is a statement about where this show's intro really is, so remember
      // it; an automatic jump is just us replaying what we already knew.
      if (!automatic && episodeId != null) {
        invoke('library.recordSkip', {
          episodeId,
          kind: marker.kind,
          startSecs: marker.startSecs,
          endSecs: marker.endSecs,
        }).catch(report('Could not remember that skip'));
      }
    },
    [episodeId],
  );

  /**
   * Whether the Up Next card is up.
   *
   * It used to also require `duration > 0` — the *player's* duration — and that is why
   * autoplay never fired on an ordinary library: a provider's VOD stream frequently
   * reports no duration at all, so the card could not appear however long the episode
   * ran. The host already refuses to produce `upNextAtSecs` without knowing a duration
   * from somewhere (the stream, the episode's runtime, or a sibling's), so having it is
   * the condition, and asking the player again was second-guessing an answer that had
   * already accounted for this.
   */
  const upNextVisible =
    !!aids?.nextEpisode &&
    aids.upNextAtSecs != null &&
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
