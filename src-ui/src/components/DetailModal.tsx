/**
 * Expanded detail view (README §8.4): backdrop with scrim, resume CTA, match %,
 * metadata pills, cast, and tabs for episodes / more like this / details.
 */
import { AnimatePresence, motion } from 'framer-motion';
import { useEffect, useState } from 'react';
import type { CatalogItem, Episode, SeriesPrefs } from '@shared/ipc';
import { useCommand } from '@/hooks/useCommand';
import { getProgress, invoke } from '@/ipc';
import { useProfile } from '@/state/profile';
import { report } from '@/lib/errors';
import { duration, progressPct, runtime } from '@/lib/format';
import { Badge, Button, IconButton, ProgressBar, Skeleton } from './Primitives';
import { Icon } from './Icon';

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
            <Hero item={item} onClose={onClose} onPlay={onPlay} />
            <Body
              item={item} tab={tab} setTab={setTab}
              season={season} setSeason={setSeason} onPlay={onPlay}
            />
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}

function Hero({
  item, onClose, onPlay,
}: { item: CatalogItem; onClose: () => void; onPlay: (i: CatalogItem) => void }) {
  const prog = item.kind === 'movie' ? getProgress('movie', item.id) : null;
  const pct = prog ? progressPct(prog.positionSecs, prog.durationSecs) : 0;

  return (
    <div style={{ position: 'relative', aspectRatio: '16 / 8', background: 'var(--surface)' }}>
      {item.backdrop && (
        <img
          src={item.backdrop} alt=""
          style={{ width: '100%', height: '100%', objectFit: 'cover' }}
        />
      )}
      <div style={{ position: 'absolute', inset: 0, background: 'var(--scrim)' }} />

      <IconButton
        icon="close" label="Close" onClick={onClose}
        style={{ position: 'absolute', top: 14, right: 14 }}
      />

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
            {pct > 0 && prog ? `Resume from ${duration(prog.positionSecs)}` : 'Play'}
          </Button>
          <IconButton icon="plus" label="Add to My List" size={46} />
          <IconButton icon="thumbUp" label="I like this" size={46} />
          <IconButton icon="record" label="Download" size={46} />
        </div>
      </div>
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
  item, tab, setTab, season, setSeason, onPlay,
}: {
  item: CatalogItem;
  tab: Tab; setTab: (t: Tab) => void;
  season: number; setSeason: (s: number) => void;
  onPlay: (i: CatalogItem, episodeId?: number) => void;
}) {
  const isSeries = item.kind === 'series';
  const tabs: Tab[] = isSeries ? ['episodes', 'similar', 'details'] : ['similar', 'details'];

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
            {isSeries && (
              <span style={{ color: 'var(--text-muted)' }}>
                {item.seasons.length} season{item.seasons.length === 1 ? '' : 's'}
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
            seriesId={item.id} seasons={item.seasons} season={season} setSeason={setSeason}
            onPlay={(epId) => onPlay(item, epId)}
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

function Episodes({
  seriesId, seasons, season, setSeason, onPlay,
}: {
  seriesId: number; seasons: number[]; season: number;
  setSeason: (s: number) => void; onPlay: (episodeId: number) => void;
}) {
  const { data, loading } = useCommand('library.episodes', { seriesId, season }, [seriesId, season]);

  return (
    <div>
      {seasons.length > 1 && (
        <select
          value={season}
          onChange={(e) => setSeason(Number(e.target.value))}
          aria-label="Season"
          style={{
            marginBottom: 'var(--sp-4)', padding: '8px 12px',
            background: 'var(--surface)', color: 'var(--text)',
            border: '1px solid var(--border-strong)', borderRadius: 'var(--r-md)',
            fontSize: 'var(--fs-md)', fontWeight: 600,
          }}
        >
          {seasons.map((s) => (
            <option key={s} value={s}>Season {s}</option>
          ))}
        </select>
      )}

      <div style={{ display: 'grid', gap: 'var(--sp-1)' }}>
        {loading && [0, 1, 2].map((i) => <Skeleton key={i} h={88} />)}
        {(data ?? []).map((ep: Episode) => (
          <button
            key={ep.id}
            onClick={() => onPlay(ep.id)}
            style={{
              display: 'grid', gridTemplateColumns: '38px 148px 1fr', gap: 'var(--sp-4)',
              alignItems: 'center', textAlign: 'left', cursor: 'pointer',
              padding: 'var(--sp-3)', borderRadius: 'var(--r-md)',
              background: 'transparent', border: 'none', color: 'inherit',
              transition: 'background var(--t-fast) var(--ease)',
            }}
            onMouseEnter={(e) => (e.currentTarget.style.background = 'var(--surface)')}
            onMouseLeave={(e) => (e.currentTarget.style.background = 'transparent')}
          >
            <div
              style={{
                fontSize: 'var(--fs-lg)', color: 'var(--text-faint)', fontWeight: 700,
                textAlign: 'center',
              }}
            >
              {ep.episode}
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
        ))}
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
