/**
 * Expanded detail view (README §8.4): backdrop with scrim, resume CTA, match %,
 * metadata pills, cast, and tabs for episodes / more like this / details.
 */
import { AnimatePresence, motion } from 'framer-motion';
import { useCallback, useEffect, useMemo, useState } from 'react';
import type { CatalogItem, Episode, EpisodeProgress, SeriesPrefs } from '@shared/ipc';
import { useCommand } from '@/hooks/useCommand';
import { getProgress, invoke } from '@/ipc';
import { useIsLiked, useMarks, useOnMyList } from '@/state/marks';
import { useProfile } from '@/state/profile';
import { report } from '@/lib/errors';
import { duration, progressPct, runtime } from '@/lib/format';
import { Badge, Button, IconButton, ProgressBar, Select, Skeleton } from './Primitives';
import { Icon } from './Icon';
import { TrailerFrame } from './TrailerFrame';

type Tab = 'episodes' | 'similar' | 'details';

export function DetailModal({
  item, onClose, onPlay,
}: {
  item: CatalogItem | null;
  onClose: () => void;
  onPlay: (i: CatalogItem, episodeId?: number) => void;
}) {
  const [tab, setTab] = useState<Tab>('episodes');
  const [season, setSeason] = useState(1);
  /*
   * One source of truth for what has been watched.
   *
   * The episode list draws the ticks and the Play button names the episode it would
   * open, and those are the same fact seen from two places. Held here so that marking an
   * episode in the list moves the button too — while they each fetched their own, the
   * button went on offering an episode that had just been ticked off.
   */
  const watch = useSeriesProgress(item?.kind === 'series' ? item.id : 0);

  useEffect(() => {
    if (!item) return;
    setTab(item.kind === 'series' ? 'episodes' : 'similar');
    setSeason(item.kind === 'series' ? (item.seasons[0] ?? 1) : 1);
  }, [item]);

  useEffect(() => {
    if (!item) return;
    const onKey = (e: KeyboardEvent) => { if (e.key === 'Escape') onClose(); };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [item, onClose]);

  return (
    <AnimatePresence>
      {item && (
        <motion.div
          initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }}
          transition={{ duration: 0.18 }}
          onClick={onClose}
          style={{
            position: 'fixed', inset: 0, zIndex: 200, overflowY: 'auto',
            background: 'rgb(0 0 0 / 0.72)', backdropFilter: 'blur(4px)',
            padding: 'var(--sp-7) var(--sp-4)',
          }}
        >
          <motion.div
            role="dialog" aria-modal="true" aria-label={item.title}
            initial={{ y: 24, scale: 0.97 }} animate={{ y: 0, scale: 1 }}
            exit={{ y: 16, scale: 0.98 }}
            transition={{ duration: 0.22, ease: [0.16, 1, 0.3, 1] }}
            onClick={(e) => e.stopPropagation()}
            style={{
              maxWidth: 940, margin: '0 auto', background: 'var(--bg-elevated)',
              borderRadius: 'var(--r-xl)', overflow: 'hidden', boxShadow: 'var(--shadow-4)',
            }}
          >
            <Hero item={item} onClose={onClose} onPlay={onPlay} watch={watch} />
            <Body
              item={item} tab={tab} setTab={setTab}
              season={season} setSeason={setSeason} onPlay={onPlay} watch={watch}
            />
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}

/**
 * The label Play should carry for a show: which episode, and whether it is a resume.
 *
 * Null for anything that is not a series, and for a series nobody has started — both
 * of which leave the button saying "Play".
 */
function useSeriesResume(item: CatalogItem, version: number): string | null {
  const profileId = useProfile((s) => s.active?.id ?? 1);
  const [label, setLabel] = useState<string | null>(null);

  useEffect(() => {
    if (item.kind !== 'series') { setLabel(null); return; }
    let live = true;
    void (async () => {
      try {
        const point = await invoke('progress.resumePoint', { profileId, seriesId: item.id });
        if (!live || !point) { if (live) setLabel(null); return; }
        const eps = await invoke('library.episodes', { seriesId: item.id });
        const ep = eps.find((e) => e.id === point.episodeId);
        if (!live || !ep) return;
        const which = `S${String(ep.season).padStart(2, '0')}E${String(ep.episode).padStart(2, '0')}`;
        setLabel(
          point.positionSecs > 0
            ? `Resume ${which} from ${duration(point.positionSecs)}`
            : `Play ${which}`,
        );
      } catch {
        // Not knowing is a reason to say "Play", not a reason to say nothing.
        if (live) setLabel(null);
      }
    })();
    return () => { live = false; };
  }, [item.kind, item.id, profileId, version]);

  return label;
}

function Hero({
  item, onClose, onPlay, watch,
}: {
  item: CatalogItem; onClose: () => void; onPlay: (i: CatalogItem) => void; watch: Watch;
}) {
  const prog = item.kind === 'movie' ? getProgress('movie', item.id) : null;
  const pct = prog ? progressPct(prog.positionSecs, prog.durationSecs) : 0;
  /*
   * What Play means for a show, said on the button before it is pressed.
   *
   * A film's resume label has always come from its own progress. A show's position is on
   * its episodes, so the button could not know — it said "Play" whether you were at the
   * pilot or halfway through series three, and then started the pilot. The host works
   * out which episode it would open; this just names it, so the button and what it does
   * agree.
   */
  const resume = useSeriesResume(item, watch.version);
  /**
   * The trailer, when asked for.
   *
   * Not on by itself, unlike the home billboard's: this modal is opened deliberately,
   * often to read the synopsis, and starting a video under the text somebody came to
   * read is an interruption rather than a flourish. With sound and with YouTube's own
   * controls, because here the trailer is the thing being watched.
   */
  const [playingTrailer, setPlayingTrailer] = useState(false);
  useEffect(() => setPlayingTrailer(false), [item.id]);
  const onMyList = useOnMyList(item);
  const liked = useIsLiked(item);
  const toggleMyList = useMarks((s) => s.toggleMyList);
  const toggleLiked = useMarks((s) => s.toggleLiked);

  return (
    <div style={{ position: 'relative', aspectRatio: '16 / 8', background: 'var(--surface)' }}>
      {item.backdrop && (
        <img
          src={item.backdrop} alt=""
          style={{ width: '100%', height: '100%', objectFit: 'cover' }}
        />
      )}
      {playingTrailer && item.trailerKey && (
        <div style={{ position: 'absolute', inset: 0, background: '#000' }}>
          <TrailerFrame
            trailerKey={item.trailerKey}
            muted={false}
            controls
            loop={false}
            title={`Trailer for ${item.title}`}
          />
        </div>
      )}
      {/* The scrim is what the title and the buttons are legible against, and it has to
          go when the trailer is the subject — a gradient over a video somebody chose to
          watch is just a dark video. */}
      {!playingTrailer && (
        <div style={{ position: 'absolute', inset: 0, background: 'var(--scrim)' }} />
      )}

      <IconButton
        icon="close" label="Close" onClick={onClose}
        style={{ position: 'absolute', top: 14, right: 14 }}
      />

      {playingTrailer ? (
        // Just a way back, pinned clear of YouTube's own controls along the bottom.
        <Button
          variant="secondary"
          icon="close"
          onClick={() => setPlayingTrailer(false)}
          style={{ position: 'absolute', left: 'var(--sp-6)', top: 'var(--sp-5)' }}
        >
          Stop trailer
        </Button>
      ) : (
      <div style={{ position: 'absolute', left: 'var(--sp-6)', right: 'var(--sp-6)', bottom: 'var(--sp-5)' }}>
        <h1
          className="text-shadow-hero"
          style={{
            margin: '0 0 var(--sp-3)', fontSize: 'var(--fs-2xl)', fontWeight: 800,
            letterSpacing: '-0.02em', maxWidth: '70%',
          }}
        >
          {item.title}
        </h1>

        {pct > 0 && prog && (
          <div style={{ maxWidth: 320, marginBottom: 'var(--sp-3)' }}>
            <ProgressBar percent={pct} height={4} />
            <div style={{ marginTop: 5, fontSize: 'var(--fs-xs)', color: 'var(--text-muted)' }}>
              {duration(prog.positionSecs)} of {duration(prog.durationSecs)}
            </div>
          </div>
        )}

        <div style={{ display: 'flex', gap: 'var(--sp-2)', alignItems: 'center', flexWrap: 'wrap' }}>
          <Button variant="primary" size="lg" icon="play" iconFilled onClick={() => onPlay(item)}>
            {pct > 0 && prog
              ? `Resume from ${duration(prog.positionSecs)}`
              : resume ?? 'Play'}
          </Button>
          {/* Only when there is one. A disabled button would be a promise the library
              cannot keep until the metadata sweep has reached this title. */}
          {item.trailerKey && (
            <Button
              variant="secondary"
              size="lg"
              icon="play"
              onClick={() => setPlayingTrailer(true)}
            >
              Trailer
            </Button>
          )}
          <IconButton
            icon={onMyList ? 'check' : 'plus'}
            label={onMyList ? `Remove ${item.title} from My List` : `Add ${item.title} to My List`}
            aria-pressed={onMyList}
            active={onMyList}
            size={46}
            onClick={() => toggleMyList(item)}
          />
          <IconButton
            icon="thumbUp"
            filled={liked}
            label={liked ? `Undo liking ${item.title}` : `I like ${item.title}`}
            aria-pressed={liked}
            active={liked}
            size={46}
            onClick={() => toggleLiked(item)}
          />
        </div>
      </div>
      )}
    </div>
  );
}

/**
 * The other copies of this title (README §13: "one card with a source picker").
 *
 * When duplicates are collapsed this is where the copies that were collapsed away go —
 * the point of collapsing is one card, not one stream, and a provider whose 4K copy
 * stalls is exactly when the HD one matters.
 */
function Sources({
  item, onPlay,
}: {
  item: CatalogItem;
  onPlay: (i: CatalogItem, episodeId?: number) => void;
}) {
  const { data } = useCommand(
    'library.alternates',
    { kind: item.kind === 'series' ? 'series' : 'movies', id: item.id },
    [item.kind, item.id],
  );
  const alternates = (data ?? []).filter((a) => a.id !== item.id);
  if (alternates.length === 0) return null;

  return (
    <div style={{ marginTop: 'var(--sp-4)' }}>
      <div
        style={{
          fontSize: 'var(--fs-xs)', fontWeight: 700, letterSpacing: '0.06em',
          textTransform: 'uppercase', color: 'var(--text-faint)', marginBottom: 6,
        }}
      >
        Also available as
      </div>
      <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap' }}>
        {alternates.map((a) => (
          <Button
            key={a.id}
            size="sm"
            icon="play"
            onClick={() => onPlay({ ...item, id: a.id, quality: a.quality })}
          >
            {a.quality ?? 'Unknown quality'}
            {a.provider ? ` · ${a.provider}` : ''}
          </Button>
        ))}
      </div>
    </div>
  );
}

function Body({
  item, tab, setTab, season, setSeason, onPlay, watch,
}: {
  watch: Watch;
  item: CatalogItem;
  tab: Tab; setTab: (t: Tab) => void;
  season: number; setSeason: (s: number) => void;
  onPlay: (i: CatalogItem, episodeId?: number) => void;
}) {
  const isSeries = item.kind === 'series';
  const tabs: Tab[] = isSeries ? ['episodes', 'similar', 'details'] : ['similar', 'details'];
  const episodes = useEpisodes(item, isSeries);

  return (
    <div style={{ padding: 'var(--sp-5) var(--sp-6) var(--sp-6)' }}>
      <div
        style={{
          display: 'grid', gridTemplateColumns: 'minmax(0, 2fr) minmax(0, 1fr)',
          gap: 'var(--sp-6)', marginBottom: 'var(--sp-5)',
        }}
      >
        <div>
          <div
            style={{
              display: 'flex', gap: 'var(--sp-3)', alignItems: 'center',
              flexWrap: 'wrap', marginBottom: 'var(--sp-3)', fontSize: 'var(--fs-sm)',
            }}
          >
            {item.match !== undefined && (
              <span style={{ color: 'var(--success)', fontWeight: 700 }}>
                {item.match}% match
              </span>
            )}
            {item.year && <span style={{ color: 'var(--text-muted)' }}>{item.year}</span>}
            {item.certification && <Badge tone="outline">{item.certification}</Badge>}
            {!isSeries && item.runtimeMins && (
              <span style={{ color: 'var(--text-muted)' }}>{runtime(item.runtimeMins)}</span>
            )}
            {/* Counted from the episodes, not from the series row. An import writes
                the row without a listing — 28,715 requests before the library was
                usable is not a trade worth making — so the row says nothing about
                seasons until somebody opens the show and the host fetches them. This
                used to read `item.seasons`, so a show sat here saying "0 seasons"
                with its episodes listed directly underneath. */}
            {isSeries && (
              <span style={{ color: 'var(--text-muted)' }}>
                {episodes.loading && episodes.seasons.length === 0
                  ? 'Loading episodes…'
                  : `${episodes.seasons.length} season${episodes.seasons.length === 1 ? '' : 's'}`}
              </span>
            )}
            {item.quality && <Badge tone={item.quality === '4K' ? 'accent' : 'neutral'}>{item.quality}</Badge>}
            <Badge tone="neutral">CC</Badge>
          </div>
          <p style={{ margin: 0, color: 'var(--text)', lineHeight: 1.6 }}>{item.overview}</p>
          <Sources item={item} onPlay={onPlay} />
        </div>

        <div style={{ fontSize: 'var(--fs-sm)', display: 'grid', gap: 'var(--sp-3)', alignContent: 'start' }}>
          <CastAndCrew kind={item.kind} id={item.id} fallback={item.cast} />
          <Meta label="Genres" value={item.genres.join(', ')} />
          {item.rating != null && (
            <Meta label="Rating" value={`${item.rating.toFixed(1)} / 10`} />
          )}
        </div>
      </div>

      <div
        role="tablist"
        style={{
          display: 'flex', gap: 'var(--sp-5)', borderBottom: '1px solid var(--border)',
          marginBottom: 'var(--sp-4)',
        }}
      >
        {tabs.map((t) => (
          <button
            key={t}
            role="tab"
            aria-selected={tab === t}
            onClick={() => setTab(t)}
            style={{
              background: 'none', border: 'none', cursor: 'pointer',
              padding: '0 0 var(--sp-3)', fontSize: 'var(--fs-md)', fontWeight: 650,
              color: tab === t ? 'var(--text)' : 'var(--text-faint)',
              borderBottom: `2px solid ${tab === t ? 'var(--accent)' : 'transparent'}`,
              marginBottom: -1, textTransform: 'capitalize',
            }}
          >
            {t === 'similar' ? 'More Like This' : t}
          </button>
        ))}
      </div>

      {tab === 'episodes' && isSeries && (
        <>
          <PlaybackPrefs seriesId={item.id} />
          <Episodes
            episodes={episodes} season={season} setSeason={setSeason}
            onPlay={(epId) => onPlay(item, epId)}
            watch={watch}
          />
        </>
      )}
      {tab === 'similar' && <Similar />}
      {tab === 'details' && <Details item={item} />}
    </div>
  );
}

/**
 * Cast and crew from enrichment, falling back to whatever the playlist supplied.
 *
 * The fallback matters: a library that has never been enriched, or a title nothing
 * matched, still has to show something rather than an empty gap where the cast was.
 */
function CastAndCrew({
  kind, id, fallback,
}: {
  kind: 'movie' | 'series';
  id: number;
  fallback: string[];
}) {
  const { data } = useCommand('metadata.credits', { kind, id }, [kind, id]);
  const credits = data ?? [];
  const cast = credits.filter((c) => c.isCast);
  const directors = credits.filter(
    (c) => !c.isCast && c.role?.toLowerCase() === 'director',
  );

  if (cast.length === 0) {
    return <Meta label="Cast" value={fallback.join(', ')} />;
  }
  return (
    <>
      <Meta
        label="Cast"
        value={cast.slice(0, 6).map((c) => c.name).join(', ')}
      />
      {directors.length > 0 && (
        <Meta
          label={directors.length === 1 ? 'Director' : 'Directors'}
          value={directors.map((c) => c.name).join(', ')}
        />
      )}
    </>
  );
}

function Meta({ label, value }: { label: string; value: string }) {
  if (!value) return null;
  return (
    <div>
      <span style={{ color: 'var(--text-faint)' }}>{label}: </span>
      <span style={{ color: 'var(--text-muted)' }}>{value}</span>
    </div>
  );
}

/**
 * Per-show playback preferences (README §9: "remember per-show whether the user
 * always skips"). They live on the show rather than in global Settings because that
 * is the scope they apply at.
 */
function PlaybackPrefs({ seriesId }: { seriesId: number }) {
  const profileId = useProfile((s) => s.active?.id ?? 1);
  const { data } = useCommand(
    'library.seriesPrefs',
    { profileId, seriesId },
    [seriesId, profileId],
  );
  const [local, setLocal] = useState<SeriesPrefs | null>(null);
  const prefs = local ?? data;

  useEffect(() => setLocal(null), [seriesId]);

  if (!prefs) return null;

  const update = (patch: Partial<SeriesPrefs>) => {
    const next = { ...prefs, ...patch };
    setLocal(next);
    invoke('library.setSeriesPrefs', { profileId, seriesId, prefs: next })
      .catch(report('Could not save that preference'));
  };

  const toggles: [keyof SeriesPrefs, string][] = [
    ['alwaysSkipIntro', 'Always skip intros'],
    ['alwaysSkipRecap', 'Always skip recaps'],
    ['autoplayNext', 'Autoplay next episode'],
  ];

  return (
    <div
      style={{
        display: 'flex', gap: 'var(--sp-2)', flexWrap: 'wrap',
        marginBottom: 'var(--sp-4)',
      }}
    >
      {toggles.map(([key, label]) => (
        <button
          key={key}
          role="switch"
          aria-checked={prefs[key]}
          onClick={() => update({ [key]: !prefs[key] } as Partial<SeriesPrefs>)}
          style={{
            display: 'inline-flex', alignItems: 'center', gap: 7,
            padding: '7px 14px', borderRadius: 'var(--r-full)', cursor: 'pointer',
            fontSize: 'var(--fs-sm)', fontWeight: 600,
            border: `1px solid ${prefs[key] ? 'transparent' : 'var(--border-strong)'}`,
            background: prefs[key] ? 'var(--accent)' : 'transparent',
            color: prefs[key] ? 'var(--accent-text)' : 'var(--text-muted)',
          }}
        >
          <Icon name={prefs[key] ? 'check' : 'plus'} size={14} />
          {label}
        </button>
      ))}
    </div>
  );
}

/** What one show's episodes turned out to be. */
interface Episodes {
  all: Episode[];
  /** The seasons those episodes are actually in, ascending. */
  seasons: number[];
  loading: boolean;
}

/**
 * Fetch a show's episodes once, for everything on the screen that needs them.
 *
 * Deliberately asks for the whole show rather than one season. The series row counts
 * the episodes already stored, so on a show whose listing has never been fetched it
 * knows nothing — and the host fetches that listing on this very call. Asking for
 * "season 1" would be a guess, and a show whose episodes are all in season 2 would
 * show nothing for ever.
 */
function useEpisodes(item: CatalogItem, isSeries: boolean): Episodes {
  const { data, loading } = useCommand(
    'library.episodes',
    { seriesId: item.id },
    [item.id],
    isSeries,
  );
  const all = useMemo(() => (isSeries ? (data ?? []) : []), [data, isSeries]);
  const seasons = useMemo(
    () => [...new Set(all.map((e: Episode) => e.season))].sort((a, b) => a - b),
    [all],
  );
  return { all, seasons, loading: isSeries && loading };
}

/** "S02E04", for a label that has to say which episode without the row around it. */
function label(ep: Episode) {
  return `S${String(ep.season).padStart(2, '0')}E${String(ep.episode).padStart(2, '0')}`;
}

/**
 * What this profile has watched of one show, and how to change it.
 *
 * Held here rather than refetched per row: one call covers the whole show, and marking
 * an episode has to move the tick, the Play button's label and the Continue Watching
 * rail together — so the state they all read has to be one thing.
 *
 * Optimistic, because the tick is the feedback. Waiting for a round trip to redraw a
 * checkbox is how a control starts feeling broken, and the failure case is a mark that
 * does not stick, which the next open corrects.
 */
function useSeriesProgress(seriesId: number) {
  const profileId = useProfile((s) => s.active?.id ?? 1);
  const [seen, setSeen] = useState<Map<number, EpisodeProgress>>(new Map());

  // Bumped on every load, so anything derived from this — the Play button's label is
  // worked out by the host — knows to ask again.
  const [version, setVersion] = useState(0);

  const load = useCallback(async () => {
    try {
      const rows = await invoke('progress.forSeries', { profileId, seriesId });
      setSeen(new Map(rows.map((r) => [r.episodeId, r])));
    } catch {
      // An episode list that cannot say what is watched is still an episode list.
      setSeen(new Map());
    }
    setVersion((v) => v + 1);
  }, [profileId, seriesId]);

  useEffect(() => { void load(); }, [load]);

  const setWatched = useCallback(async (episodeId: number, watched: boolean) => {
    setSeen((prev) => {
      const next = new Map(prev);
      const was = next.get(episodeId);
      next.set(episodeId, {
        episodeId,
        positionSecs: was?.positionSecs ?? 0,
        durationSecs: was?.durationSecs ?? 0,
        completed: watched,
      });
      return next;
    });
    try {
      await invoke('progress.setWatched', { profileId, kind: 'episode', id: episodeId, watched });
    } finally {
      await load();
    }
  }, [profileId, load]);

  return { get: (id: number) => seen.get(id), setWatched, version };
}

type Watch = ReturnType<typeof useSeriesProgress>;

function Episodes({
  episodes, season, setSeason, onPlay, watch,
}: {
  episodes: Episodes; season: number;
  setSeason: (s: number) => void; onPlay: (episodeId: number) => void;
  watch: Watch;
}) {
  const { seasons: choices, loading } = episodes;
  // Filtering here rather than asking the host again: the whole show is already in
  // hand, and a round trip per season change would be a spinner for no reason.
  const data = useMemo(
    () => (choices.length > 1 ? episodes.all.filter((e) => e.season === season) : episodes.all),
    [episodes.all, choices.length, season],
  );

  return (
    <div>
      {choices.length > 1 && (
        <Select
          label="Season"
          options={choices.map((s) => ({ value: String(s), label: `Season ${s}` }))}
          value={String(season)}
          onChange={(v) => setSeason(Number(v ?? choices[0]))}
          style={{ marginBottom: 'var(--sp-4)' }}
        />
      )}

      <div style={{ display: 'grid', gap: 'var(--sp-1)' }}>
        {loading && [0, 1, 2].map((i) => <Skeleton key={i} h={88} />)}
        {!loading && (data ?? []).length === 0 && (
          <div style={{ padding: 'var(--sp-5)', color: 'var(--text-muted)' }}>
            This provider listed the show but returned no episodes for it.
          </div>
        )}
        {(data ?? []).map((ep: Episode) => {
        const seen = watch.get(ep.id);
        const watched = seen?.completed ?? false;
        const partway = !watched && seen != null && seen.durationSecs > 0
          ? progressPct(seen.positionSecs, seen.durationSecs)
          : 0;
        return (
        <div
          key={ep.id}
          style={{ display: 'flex', alignItems: 'center', borderRadius: 'var(--r-md)' }}
          onMouseEnter={(e) => (e.currentTarget.style.background = 'var(--surface)')}
          onMouseLeave={(e) => (e.currentTarget.style.background = 'transparent')}
        >
          <button
            onClick={() => onPlay(ep.id)}
            style={{
              flex: 1, minWidth: 0,
              display: 'grid', gridTemplateColumns: '38px 148px 1fr', gap: 'var(--sp-4)',
              alignItems: 'center', textAlign: 'left', cursor: 'pointer',
              padding: 'var(--sp-3)', borderRadius: 'var(--r-md)',
              background: 'transparent', border: 'none', color: 'inherit',
              // Watched episodes step back rather than disappear: the list is still how
              // you get to one you want to see again.
              opacity: watched ? 0.55 : 1,
              transition: 'opacity var(--t-fast) var(--ease)',
            }}
          >
            <div
              style={{
                fontSize: 'var(--fs-lg)', color: 'var(--text-faint)', fontWeight: 700,
                textAlign: 'center',
              }}
            >
              {watched ? <Icon name="check" size={18} /> : ep.episode}
            </div>
            <div style={{ position: 'relative', borderRadius: 'var(--r-sm)', overflow: 'hidden' }}>
              {ep.still && (
                <img
                  src={ep.still} alt=""
                  style={{ width: '100%', aspectRatio: '16/9', objectFit: 'cover', display: 'block' }}
                />
              )}
              <div
                style={{
                  position: 'absolute', inset: 0, display: 'grid', placeItems: 'center',
                  background: 'rgb(0 0 0 / 0.25)', color: '#fff',
                }}
              >
                <Icon name="play" size={22} filled />
              </div>
              {partway > 0 && (
                <div
                  style={{
                    position: 'absolute', left: 0, right: 0, bottom: 0, height: 3,
                    background: 'rgb(255 255 255 / 0.25)',
                  }}
                >
                  <div style={{ width: `${partway}%`, height: '100%', background: 'var(--accent)' }} />
                </div>
              )}
            </div>
            <div style={{ minWidth: 0 }}>
              <div
                style={{
                  display: 'flex', justifyContent: 'space-between', gap: 'var(--sp-3)',
                  marginBottom: 3,
                }}
              >
                <strong style={{ fontSize: 'var(--fs-md)' }}>{ep.title}</strong>
                <span style={{ color: 'var(--text-faint)', fontSize: 'var(--fs-sm)' }}>
                  {runtime(ep.runtimeMins)}
                </span>
              </div>
              <div
                style={{
                  color: 'var(--text-muted)', fontSize: 'var(--fs-sm)',
                  display: '-webkit-box', WebkitLineClamp: 2, WebkitBoxOrient: 'vertical',
                  overflow: 'hidden',
                }}
              >
                {ep.overview}
              </div>
            </div>
          </button>

          {/* A sibling of the row, not a child: a button inside a button is invalid
              markup, and the whole row already means "play this". */}
          {/* One control in two states rather than two icons: a ✕ beside a row reads as
              "delete this", which is not what unmarking does. The same check, filled
              when it is true, is the vocabulary the Keep heart already uses. */}
          <IconButton
            size={34}
            icon="check"
            filled={watched}
            active={watched}
            label={watched ? `Mark ${label(ep)} as unplayed` : `Mark ${label(ep)} as played`}
            onClick={() => void watch.setWatched(ep.id, !watched)}
          />
        </div>
        );
        })}
      </div>
    </div>
  );
}

function Similar() {
  const { data } = useCommand('library.movies', { sort: 'rating', limit: 9, offset: 12 }, []);
  return (
    <div
      style={{
        display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(150px, 1fr))',
        gap: 'var(--sp-3)',
      }}
    >
      {(data ?? []).map((m) => (
        <div key={m.id} style={{ borderRadius: 'var(--r-md)', overflow: 'hidden', background: 'var(--surface)' }}>
          {m.backdrop && (
            <img src={m.backdrop} alt="" style={{ width: '100%', aspectRatio: '16/9', objectFit: 'cover', display: 'block' }} />
          )}
          <div style={{ padding: 'var(--sp-3)' }}>
            <div style={{ fontWeight: 650, fontSize: 'var(--fs-sm)', marginBottom: 4 }}>{m.title}</div>
            <div style={{ fontSize: 'var(--fs-xs)', color: 'var(--text-faint)' }}>
              {m.year} · {runtime(m.runtimeMins)}
            </div>
          </div>
        </div>
      ))}
    </div>
  );
}

function Details({ item }: { item: CatalogItem }) {
  const rows: [string, string][] = [
    ['Title', item.title],
    ['Year', String(item.year ?? '—')],
    ['Genres', item.genres.join(', ') || '—'],
    ['Certification', item.certification ?? '—'],
    ['Quality', item.quality ?? '—'],
    ['Container', 'matroska (mkv)'],
    ['Video', 'HEVC Main 10 · 3840x2160 · 23.976 fps'],
    ['Audio', 'E-AC-3 5.1 @ 640 kbps · AAC 2.0'],
    ['Subtitles', 'SRT (eng), ASS (eng SDH)'],
    ['Source', 'example.com'],
  ];
  return (
    <dl style={{ margin: 0, display: 'grid', gridTemplateColumns: '160px 1fr', gap: 'var(--sp-2) var(--sp-4)', fontSize: 'var(--fs-sm)' }}>
      {rows.map(([k, v]) => (
        <div key={k} style={{ display: 'contents' }}>
          <dt style={{ color: 'var(--text-faint)' }}>{k}</dt>
          <dd style={{ margin: 0, color: 'var(--text-muted)' }}>{v}</dd>
        </div>
      ))}
    </dl>
  );
}
