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
  /**
   * Which episode these aids are about, or null when what is playing is not one.
   *
   * Exported so the screen can key per-episode state on it. `SkipButton` keeps a set of
   * regions it has already auto-skipped -- so that seeking back into an intro does not
   * yank the viewer forward again -- and that set is per episode, which nothing told it
   * (R-149).
   */
  episodeId: number | null;
  /** The marker under the playhead right now, if any. */
  activeMarker: SkipMarker | null;
  /** Whether the current marker should be skipped without being asked. */
  autoSkip: boolean;
  upNextVisible: boolean;
  skip: (marker: SkipMarker, automatic: boolean) => void;
  playNext: () => void;
  dismissUpNext: () => void;
}

/**
 * Which marker sources may fire without being asked.
 *
 * Mirrors `aurora_core::markers::MarkerSource::may_auto_skip`, the host's half of the
 * same rule. A map rather than `!== 'convention'`, for two reasons: an unrecognised
 * source reads as `undefined` and so is excluded rather than admitted, and
 * `Record<SkipMarker['source'], boolean>` is exhaustiveness-checked — adding a source to
 * the contract will not compile until somebody decides which side of this line it is on.
 */
const MAY_AUTO_SKIP: Record<SkipMarker['source'], boolean> = {
  chapters: true,
  user: true,
  learned: true,
  // Placed by the clock rather than by evidence. See the comment on `autoSkip`.
  convention: false,
};

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
  /**
   * Whether the file itself is open, which is when mpv knows its chapters.
   *
   * `player.play` returns as soon as `loadfile` is accepted, so the episode id arrives
   * in the mirrored state while the demuxer is still working: `chapter-list/count` is 0
   * and `library.syncChapters` stores nothing. mpv sets this status from its own
   * `file-loaded` event -- the same place it refreshes the track list -- so it is the
   * first moment there is a chapter list to read.
   */
  const loaded = player?.status === 'playing' || player?.status === 'paused';

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
  /**
   * What has been asked, as the state the answer depended on.
   *
   * Three things unlock evidence that was not there a moment ago, and asking before any
   * of them lands gets an honest but empty answer:
   *
   *   * the episode (a different one has different everything),
   *   * a real duration (it places a conventional credits marker),
   *   * the file being open (mpv has no chapter list until then -- and that is the one
   *     that kept Skip Intro off the screen entirely).
   *
   * The chapter sync used to be tied to the duration alone, which is only a proxy for
   * "the file is open" and the wrong one: a provider's VOD stream frequently reports no
   * duration at all, sometimes never. On those -- and `durationSecs: 0` is the ordinary
   * case here -- the only `syncChapters` call ever made was the one issued before the
   * demuxer had read the file, so a file whose chapters named its intro had them stored
   * from nowhere and `skip_markers` stayed empty. Intro and recap are evidence-only by
   * design (`markers::conventional` places neither), so with the chapter path dead there
   * was nothing left to put Skip Intro on screen -- and nothing a viewer could press to
   * teach it either, because `library.recordSkip` is reached from that button.
   *
   * Still bounded, which is what the pair it replaces was for: the key is one id and two
   * booleans, so it is at most four calls for an episode and in practice two.
   * `position` is deliberately not in it.
   */
  const fetchedFor = useRef<string | null>(null);
  /** Where a skip we issued is heading, so it is not mistaken for the viewer's own. */
  const ourSeek = useRef<number | null>(null);
  /**
   * Bumped when the host takes a seek as evidence, which is the one thing that can add
   * a marker while an episode is already playing. In the stage key below, so the markers
   * are re-read and the button appears for the rest of this episode too — not only from
   * the next one onwards.
   */
  const [evidence, setEvidence] = useState(0);
  useEffect(() => {
    if (episodeId == null) {
      setAids(null);
      fetchedFor.current = null;
      return;
    }
    const stage = `${episodeId}:${duration > 0 ? 'd' : '-'}:${loaded ? 'l' : '-'}:${evidence}`;
    if (fetchedFor.current === stage) return;
    // Only when the episode itself changes: a refetch must not un-dismiss a card the
    // viewer has already waved away.
    if (!fetchedFor.current?.startsWith(`${episodeId}:`)) setDismissed(null);
    fetchedFor.current = stage;
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
  }, [episodeId, duration, loaded, profileId, evidence]);

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
   * The source has to be one of the evidence-backed ones, named. A conventional marker
   * is placed by the clock rather than by evidence, so it is a button somebody may press
   * and never something that fires on its own: auto-skipping a guess would mean a wrong
   * runtime silently cutting the last minute of an episode — which is how you lose the
   * final scene of a finale.
   *
   * Looked up in a table rather than asked as `!== 'convention'`. The negative form
   * says yes to anything it does not recognise, and the host's own fallback did the same
   * thing — an unparseable `source` column was read back as `learned`, which auto-skips
   * — so the two guards were one decision made twice from the same wrong value, with no
   * second line of defence. An unrecognised source now declines to fire by itself, which
   * is the direction that costs a button press rather than a final scene (R-45).
   */
  const autoSkip =
    !!activeMarker &&
    !!aids &&
    MAY_AUTO_SKIP[activeMarker.source] === true &&
    ((activeMarker.kind === 'intro' && aids.prefs.alwaysSkipIntro) ||
      (activeMarker.kind === 'recap' && aids.prefs.alwaysSkipRecap));

  const skip = useCallback(
    (marker: SkipMarker, automatic: boolean) => {
      // Where we are about to land, so the seek watcher below does not read our own
      // jump back as the viewer making a fresh statement about this show's intro.
      ourSeek.current = marker.endSecs;
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

  /**
   * A forward jump the viewer made themselves, offered to the host as intro evidence.
   *
   * This is what closes the loop the comment above describes. `library.recordSkip` is
   * reached from the Skip button, the button only exists where a marker already does,
   * and the only thing that could create the first intro marker was a chapter list —
   * which a provider's VOD stream almost never has. So on a real library Skip Intro
   * could not appear and could not be taught. Dragging past the titles is the manual
   * skip the design's second tier is about, and it was being thrown away.
   *
   * The host decides whether a jump is evidence; this only reports one. Guards here are
   * about *whose* jump it was:
   *
   *   * `ourSeek` — a skip we issued is us replaying what we already knew, not a new
   *     statement, exactly as `skip` already distinguishes automatic from pressed.
   *   * the first sample after the file opens is never compared against anything, so
   *     resuming part-way through cannot read as a jump from zero. Resume is passed
   *     into the open call and the host starts there, so the playhead is already at the
   *     resume point by the time the status says playing.
   *
   * A seek backwards, a nudge, a scrub across the episode and a jump outside the window
   * an intro can occupy are all refused by the host, so none of that is duplicated here.
   */
  const lastPosition = useRef<number | null>(null);
  useEffect(() => {
    if (episodeId == null || !loaded) {
      lastPosition.current = null;
      return;
    }
    const previous = lastPosition.current;
    lastPosition.current = position;
    if (previous == null) return;

    const jumped = position - previous;
    if (jumped < 5) return;

    // Ours rather than theirs: `skip` seeks to `marker.endSecs`, so a landing within a
    // couple of seconds of where we aimed is the jump we asked for.
    const aimed = ourSeek.current;
    if (aimed != null && Math.abs(position - aimed) < 2) {
      ourSeek.current = null;
      return;
    }

    void invoke('library.observeSeek', {
      episodeId,
      fromSecs: previous,
      toSecs: position,
      durationSecs: duration,
    })
      // Refetch only when it was taken, so an ordinary scrub costs nothing.
      .then((taken) => {
        if (taken) setEvidence((n) => n + 1);
      })
      // Not fatal: the worst case is the library learns nothing from this jump.
      .catch(() => {});
  }, [position, episodeId, loaded, duration]);

  return {
    aids, episodeId, activeMarker, autoSkip, upNextVisible, skip, playNext, dismissUpNext,
  };
}
