/**
 * The recordings library (README §7.7).
 *
 * Three tabs rather than one list, because the three groups answer different
 * questions: "what can I watch", "what is Aurora about to do", and "what went wrong".
 * Mixing them means the failures are the easiest thing to miss.
 */
import { useCallback, useEffect, useState } from 'react';
import type { Recording, RecordingConflict, RecordingRule } from '@shared/ipc';
import { Icon } from '@/components/Icon';
import {
  Badge, Button, ChannelTile, EmptyState, IconButton, ProgressBar, Skeleton, artHue,
} from '@/components/Primitives';
import { useCommand } from '@/hooks/useCommand';
import { invoke, onDvrTick } from '@/ipc';
import { report } from '@/lib/errors';
import { bytes, dayLabel, duration } from '@/lib/format';
import { useClockTime, useUntilLabel } from '@/hooks/useClockTime';

type Tab = 'recorded' | 'scheduled' | 'rules';

const TABS: { id: Tab; label: string }[] = [
  { id: 'recorded', label: 'Recorded' },
  { id: 'scheduled', label: 'Scheduled' },
  { id: 'rules', label: 'Series rules' },
];

const WEEKDAYS = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun'];

/**
 * `onPlay` is required, not optional.
 *
 * It used to be optional and the only mount supplied nothing, so `onPlay?.(rec)` on
 * every completed recording was a no-op -- valid TypeScript, invisible to the type
 * checker and to a sweep for handler-less buttons, and the feature behind it did not
 * exist at all (R-118).
 */
export function RecordingsPage({ onPlay }: { onPlay: (r: Recording) => void }) {
  const [tab, setTab] = useState<Tab>('recorded');
  const [nonce, setNonce] = useState(0);
  const refresh = useCallback(() => setNonce((n) => n + 1), []);

  const { data: all, loading } = useCommand('dvr.list', {}, [nonce]);
  const { data: rules } = useCommand('dvr.rules', undefined, [nonce]);
  const { data: conflicts } = useCommand('dvr.conflicts', undefined, [nonce]);
  const { data: storage } = useCommand('dvr.storage', undefined, [nonce]);

  // A recording starting or finishing changes this page, and the host already tells
  // us when that happens — polling would only make it slower to notice.
  useEffect(() => onDvrTick(() => refresh()), [refresh]);

  const recordings = all ?? [];
  const recorded = recordings.filter((r) => r.state === 'completed');
  const upcoming = recordings.filter((r) => r.state === 'scheduled' || r.state === 'recording');
  const problems = recordings.filter((r) => r.state === 'failed' || r.state === 'skipped');

  const counts: Record<Tab, number> = {
    recorded: recorded.length,
    scheduled: upcoming.length,
    rules: (rules ?? []).length,
  };

  return (
    <div style={{ padding: '4px 36px var(--sp-6)' }}>
      <div style={headerRow}>
        <h1 className="dl-page-title">Recordings</h1>
        <div role="tablist" aria-label="Recordings" className="dl-seg">
          {TABS.map((t) => (
            <button
              key={t.id}
              type="button"
              role="tab"
              aria-selected={tab === t.id}
              data-active={tab === t.id}
              data-variant={tab === t.id ? 'primary' : 'ghost'}
              onClick={() => setTab(t.id)}
            >
              {t.label}
              <span style={{ opacity: 0.65 }}>{counts[t.id]}</span>
            </button>
          ))}
        </div>
        {storage && (
          <div style={{ marginLeft: 'auto' }}>
            <StorageMeter
              usedBytes={storage.usedBytes}
              quotaBytes={storage.quotaBytes}
              folder={storage.folder}
            />
          </div>
        )}
      </div>

      {(conflicts ?? []).length > 0 && tab !== 'recorded' && (
        <ConflictNotice conflicts={conflicts ?? []} recordings={recordings} />
      )}

      {loading && (
        <div style={{ display: 'grid', gap: 'var(--sp-2)' }}>
          {Array.from({ length: 6 }, (_, i) => <Skeleton key={i} h={72} r={18} />)}
        </div>
      )}

      {!loading && tab === 'recorded' && (
        recorded.length === 0 ? (
          <EmptyState
            icon="record"
            title="Nothing recorded yet"
            body="Press Record on anything in the guide, or set up a series rule and Aurora will keep up with it for you."
          />
        ) : (
          <div style={cardGrid}>
            {recorded.map((r) => (
              <RecordedRow
                key={r.id}
                rec={r}
                onChanged={refresh}
                onPlay={onPlay}
              />
            ))}
          </div>
        )
      )}

      {/* Daylight's "Coming up" panel under the recorded grid: a glance at what is
          next, with the full list and its controls one tab away. */}
      {!loading && tab === 'recorded' && upcoming.length > 0 && (
        <ComingUp
          upcoming={upcoming.slice(0, 5)}
          onRules={() => setTab('rules')}
        />
      )}

      {!loading && tab === 'scheduled' && (
        <>
          {upcoming.length === 0 ? (
            <EmptyState
              icon="clock"
              title="Nothing scheduled"
              body="Recordings you set from the guide show up here, with a countdown and anything that clashes."
            />
          ) : (
            <div style={listStyle}>
              {upcoming.map((r) => (
                <ScheduledRow
                  key={r.id}
                  rec={r}
                  onChanged={refresh}
                />
              ))}
            </div>
          )}

          {problems.length > 0 && (
            <>
              <h2 style={sectionHeading}>Didn&rsquo;t record</h2>
              <div style={listStyle}>
                {problems.map((r) => (
                  <ProblemRow
                    key={r.id}
                    rec={r}
                    onChanged={refresh}
                  />
                ))}
              </div>
            </>
          )}
        </>
      )}

      {!loading && tab === 'rules' && (
        <RulesTab rules={rules ?? []} onChanged={refresh} />
      )}
    </div>
  );
}

/* ── Storage ──────────────────────────────────────────────────────────────── */

function StorageMeter({
  usedBytes, quotaBytes, folder,
}: {
  usedBytes: number;
  quotaBytes: number;
  folder: string;
}) {
  const pct = quotaBytes > 0 ? (usedBytes / quotaBytes) * 100 : 0;
  return (
    <div className="dl-panel" style={{ minWidth: 280, padding: '12px 18px', borderRadius: 'var(--r-lg)' }}>
      <div
        style={{
          display: 'flex', justifyContent: 'space-between', gap: 12, marginBottom: 6,
          fontSize: 'var(--fs-sm)', fontWeight: 600, color: 'var(--text-muted)',
        }}
      >
        <span>Storage</span>
        <span style={{ color: 'var(--text)', fontWeight: 700 }}>
          {bytes(usedBytes)}
          {quotaBytes > 0 && <> of {bytes(quotaBytes)}</>}
        </span>
      </div>
      {quotaBytes > 0 && (
        <ProgressBar percent={pct} height={6} label="Recording storage used" />
      )}
      <div
        style={{
          fontSize: 'var(--fs-xs)', color: 'var(--text-faint)', marginTop: 6,
          whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis', maxWidth: 320,
        }}
        title={folder}
      >
        {folder}
      </div>
    </div>
  );
}

/* ── Conflicts ────────────────────────────────────────────────────────────── */

function ConflictNotice({
  conflicts, recordings,
}: {
  conflicts: RecordingConflict[];
  recordings: Recording[];
}) {
  // Bound to the 12/24-hour preference, shadowing the pure import (R-136).
  const clockTime = useClockTime();
  const titleOf = (id: number) => recordings.find((r) => r.id === id)?.title ?? `#${id}`;
  return (
    <div role="status" style={noticeStyle}>
      <Icon name="info" size={18} />
      <div>
        <strong>
          {conflicts.length === 1 ? 'A recording clashes' : `${conflicts.length} clashes`}
        </strong>
        {conflicts.slice(0, 2).map((c) => (
          <div key={`${c.start}-${c.stop}`} style={{ color: 'var(--text-muted)', marginTop: 2 }}>
            {dayLabel(c.start)} {clockTime(c.start)}–{clockTime(c.stop)}: {c.slotIds.length} at
            once, {c.overBy} more than your subscription allows.{' '}
            {/* The tail of the priority-ordered list is what loses, so name it. */}
            <em>{titleOf(c.slotIds[c.slotIds.length - 1] ?? 0)}</em> will be dropped.
          </div>
        ))}
      </div>
    </div>
  );
}

/**
 * Run a host command from a click, and say so when it fails.
 *
 * Six call sites in this file were `onClick={async () => { await invoke(...); onChanged(); }}`
 * with no `catch`. A rejected command was an unhandled promise rejection: it reached the
 * console, the row did not change, and the viewer was left pressing a button that
 * appeared to do nothing. `onChanged` is only called when the command actually worked,
 * so a failed delete does not refetch as though it had (R-119).
 */
async function guard(
  fn: () => Promise<unknown>,
  whenItFails: string,
  after: () => void,
): Promise<void> {
  try {
    await fn();
  } catch (e) {
    report(whenItFails)(e);
    return;
  }
  after();
}

/* ── Rows ─────────────────────────────────────────────────────────────────── */

function RowShell({
  children, accent, testId,
}: {
  children: React.ReactNode;
  accent?: string;
  /** So a journey can assert about one row rather than about the whole screen. */
  testId?: string;
}) {
  return (
    <div
      data-testid={testId}
      style={{
        display: 'flex', alignItems: 'center', gap: 'var(--sp-3)',
        padding: '14px 16px', background: 'var(--surface)',
        borderRadius: 'var(--r-lg)', boxShadow: accent
          ? `inset 4px 0 0 ${accent}, var(--shadow-1)`
          : 'var(--shadow-1)',
      }}
    >
      {children}
    </div>
  );
}

/**
 * Takes the channel rather than its name, so the row can carry its logo.
 *
 * Every other list in the app is something to look at — posters on the home rails, logos
 * down the guide — and this one was four lines of grey text. The page already holds the
 * channels it is naming, so the mark costs nothing to show and makes a long list scannable
 * without reading it.
 */
function TitleBlock({ rec }: { rec: Recording }) {
  // Bound to the 12/24-hour preference, shadowing the pure import (R-136).
  const clockTime = useClockTime();
  const ep = rec.season != null && rec.episode != null
    ? `S${String(rec.season).padStart(2, '0')}E${String(rec.episode).padStart(2, '0')}`
    : null;
  return (
    <div style={{ minWidth: 0, flex: 1, display: 'flex', alignItems: 'center', gap: 'var(--sp-3)' }}>
      <ChannelTile
        name={rec.channelName ?? rec.title} number={null} logo={rec.channelLogo} size={40}
      />
      <div style={{ minWidth: 0, flex: 1 }}>
        <div style={{ fontWeight: 800, whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' }}>
          {rec.title}
          {ep && <span style={{ color: 'var(--text-faint)', fontWeight: 500 }}> · {ep}</span>}
        </div>
        <div style={{ fontSize: 'var(--fs-sm)', color: 'var(--text-muted)' }}>
          {rec.channelName && <>{rec.channelName} · </>}
          {dayLabel(rec.airStart)} {clockTime(rec.airStart)}–{clockTime(rec.airStop)}
        </div>
      </div>
    </div>
  );
}

function RecordedRow({
  rec, onChanged, onPlay,
}: {
  rec: Recording;
  onChanged: () => void;
  onPlay: (r: Recording) => void;
}) {
  const [confirming, setConfirming] = useState(false);
  const clockTime = useClockTime();
  // Caught, because a click that fails has to say so. Six call sites in this file
  // invoked a host command from an `onClick` and never caught: a rejected `dvr.delete`
  // or `dvr.cancel` was an unhandled promise rejection, which reaches the console and
  // nothing else -- the row simply did not change and the viewer was left pressing a
  // button that appeared to do nothing (R-118's shape, R-119's scope).
  const act = async (fn: () => Promise<unknown>) => {
    try {
      await fn();
    } catch (e) {
      report('That did not work')(e);
      return;
    }
    onChanged();
  };

  /*
   * Deleting a recording erases the file. There is no undo and no recycle bin, and the
   * button that does it sat one 32px target away from Keep — the control whose entire
   * purpose is to say "do not lose this". So it asks first, in the row, the same two-step
   * the provider editor uses for the other irreversible thing in this app. Cancelling a
   * scheduled recording and dismissing a failed one still go in one click, because
   * neither has a file to lose.
   */
  return (
    <div
      className="dl-panel"
      style={{
        padding: 14, display: 'flex', flexDirection: 'column', gap: 10, minWidth: 0,
        boxShadow: confirming ? 'inset 0 0 0 2px var(--danger)' : 'var(--shadow-1)',
      }}
    >
      {/* The art: the channel's colour, its logo when it has one, and the tags. */}
      <div
        style={{
          position: 'relative', aspectRatio: '2 / 1', borderRadius: 'var(--r-lg)',
          overflow: 'hidden', background: artHue(rec.channelName ?? rec.title),
          display: 'grid', placeItems: 'center',
        }}
      >
        {rec.channelLogo && (
          <img
            src={rec.channelLogo} alt=""
            style={{ width: '34%', height: '50%', objectFit: 'contain', opacity: 0.9 }}
          />
        )}
        <div style={{ position: 'absolute', top: 12, left: 12, display: 'flex', gap: 6 }}>
          {rec.reason && (
            // A completed-but-short recording still plays. Saying so beats letting the
            // viewer discover it 40 minutes in.
            <span title={rec.reason} className="dl-chip" style={ON_ART_CHIP}>Cut short</span>
          )}
          {!rec.watched && <Badge tone="new">New</Badge>}
          {rec.keep && <span className="dl-chip" style={ON_ART_CHIP}>Kept</span>}
        </div>
      </div>

      <div style={{ minWidth: 0 }}>
        <div
          style={{
            fontWeight: 800, fontSize: 'var(--fs-lg)', whiteSpace: 'nowrap',
            overflow: 'hidden', textOverflow: 'ellipsis',
          }}
        >
          {rec.title}
        </div>
        <div
          style={{
            fontSize: 'var(--fs-sm)', color: 'var(--text-muted)', whiteSpace: 'nowrap',
            overflow: 'hidden', textOverflow: 'ellipsis',
          }}
        >
          {rec.channelName && <>{rec.channelName} · </>}
          {dayLabel(rec.airStart)} {clockTime(rec.airStart)}–{clockTime(rec.airStop)}
        </div>
      </div>

      {confirming ? (
        <div style={{ display: 'grid', gap: 8 }}>
          <span style={{ fontSize: 'var(--fs-sm)', fontWeight: 700, color: 'var(--danger)' }}>
            Delete permanently, freeing {bytes(rec.bytes)}?
          </span>
          <div style={{ display: 'flex', gap: 8 }}>
            <Button
              size="sm"
              variant="danger"
              onClick={() => act(() => invoke('dvr.delete', { id: rec.id }))}
            >
              Delete
            </Button>
            <Button size="sm" onClick={() => setConfirming(false)}>Keep</Button>
          </div>
        </div>
      ) : (
        <div style={{ display: 'flex', alignItems: 'center', gap: 6, marginTop: 'auto' }}>
          <span
            style={{
              fontSize: 'var(--fs-xs)', fontWeight: 700, color: 'var(--text-muted)',
              whiteSpace: 'nowrap', marginRight: 'auto',
            }}
          >
            {duration(rec.durationSecs)} · {bytes(rec.bytes)}
          </span>
          <IconButton
            size={34}
            icon="heart"
            filled={rec.keep}
            active={rec.keep}
            label={rec.keep ? 'Allow deleting to free space' : 'Keep — never delete for space'}
            onClick={() => act(() => invoke('dvr.setKeep', { id: rec.id, value: !rec.keep }))}
          />
          <IconButton
            size={34}
            icon="close"
            label={`Delete ${rec.title}`}
            onClick={() => setConfirming(true)}
          />
          <Button size="sm" variant="primary" icon="play" iconFilled onClick={() => onPlay(rec)}>
            Play
          </Button>
        </div>
      )}
    </div>
  );
}

/**
 * The white "Coming up" panel: each scheduled recording with a coral REC chip, the
 * channel, when, and whether a series rule or a one-off asked for it.
 */
function ComingUp({ upcoming, onRules }: { upcoming: Recording[]; onRules: () => void }) {
  const clockTime = useClockTime();
  return (
    <section
      aria-labelledby="coming-up"
      className="dl-panel"
      style={{ marginTop: 16, padding: '20px 24px 12px' }}
    >
      <div style={{ display: 'flex', alignItems: 'center', marginBottom: 8 }}>
        <h2
          id="coming-up"
          className="dl-display"
          style={{ margin: 0, fontSize: 'var(--fs-2xl)', color: 'var(--text)' }}
        >
          Coming up
        </h2>
        <Button size="sm" variant="outline" onClick={onRules} style={{ marginLeft: 'auto' }}>
          Series rules
        </Button>
      </div>
      {upcoming.map((r) => (
        <div
          key={r.id}
          style={{
            display: 'grid', gridTemplateColumns: 'auto minmax(0, 1.4fr) minmax(0, 1fr) auto auto',
            alignItems: 'center', gap: 16, padding: '12px 0',
            borderBottom: '1px solid var(--border)', fontSize: 'var(--fs-md)',
          }}
        >
          <span className="dl-chip" data-tone="live">REC</span>
          <span style={{ fontWeight: 800, whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' }}>
            {r.subTitle ? `${r.title}: ${r.subTitle}` : r.title}
          </span>
          <span style={{ color: 'var(--text-muted)', fontWeight: 600, whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' }}>
            {r.channelName ?? ''}
          </span>
          <span style={{ fontWeight: 800, whiteSpace: 'nowrap' }}>
            {dayLabel(r.airStart)} {clockTime(r.airStart)}–{clockTime(r.airStop)}
          </span>
          <span className="dl-chip">{r.ruleId != null ? 'Series rule' : 'Once'}</span>
        </div>
      ))}
    </section>
  );
}

function ScheduledRow({
  rec, onChanged,
}: {
  rec: Recording;
  onChanged: () => void;
}) {
  // Bound to the 12/24-hour preference, shadowing the pure import (R-136).
  const untilLabel = useUntilLabel();
  const live = rec.state === 'recording';
  const elapsed = live ? Math.max(0, Math.floor(Date.now() / 1000) - rec.start) : 0;
  const pct = live ? (elapsed / Math.max(1, rec.stop - rec.start)) * 100 : 0;

  return (
    <RowShell accent={live ? 'var(--live)' : undefined} testId="scheduled-row">
      <TitleBlock rec={rec} />

      <div style={{ display: 'flex', alignItems: 'center', gap: 'var(--sp-2)' }}>
        {live ? (
          <>
            <Badge tone="live">
              <Icon name="record" size={11} filled /> Recording
            </Badge>
            <div style={{ width: 120 }}>
              {/* The only thing on the row that says how far through it is: the text
                  beside it is a byte count, not a proportion. */}
              <ProgressBar percent={pct} label={`Recording ${rec.title}`} />
            </div>
            <span style={{ fontSize: 'var(--fs-sm)', color: 'var(--text-faint)' }}>
              {bytes(rec.liveBytes ?? rec.bytes)}
            </span>
          </>
        ) : (
          <span style={{ fontSize: 'var(--fs-sm)', color: 'var(--text-muted)', whiteSpace: 'nowrap' }}>
            {untilLabel(rec.start)}
          </span>
        )}
        <Button
          size="sm"
          variant="ghost"
          onClick={() => {
            void guard(
              () => invoke('dvr.cancel', { id: rec.id }),
              'Could not stop that recording',
              onChanged,
            );
          }}
        >
          {live ? 'Stop' : 'Cancel'}
        </Button>
      </div>
    </RowShell>
  );
}

function ProblemRow({
  rec, onChanged,
}: {
  rec: Recording;
  onChanged: () => void;
}) {
  return (
    <RowShell accent="var(--danger)">
      <div style={{ minWidth: 0, flex: 1 }}>
        <TitleBlock rec={rec} />
        {rec.reason && (
          <div style={{ fontSize: 'var(--fs-sm)', color: 'var(--danger)', marginTop: 2 }}>
            {rec.reason}
          </div>
        )}
      </div>
      <Badge tone="outline">{rec.state === 'skipped' ? 'Skipped' : 'Failed'}</Badge>
      <IconButton
        size={32}
        icon="close"
        label={`Dismiss ${rec.title}`}
        onClick={() => {
          void guard(
            () => invoke('dvr.delete', { id: rec.id }),
            'Could not dismiss that recording',
            onChanged,
          );
        }}
      />
    </RowShell>
  );
}

/* ── Series rules ─────────────────────────────────────────────────────────── */

function RulesTab({
  rules, onChanged,
}: {
  rules: RecordingRule[];
  onChanged: () => void;
}) {
  if (rules.length === 0) {
    return (
      <EmptyState
        icon="stack"
        title="No series rules"
        body="A series rule records every episode of a show as it airs. Set one from the Record button in the guide."
      />
    );
  }
  return (
    <div style={listStyle}>
      {rules.map((rule) => (
        <RuleRow key={rule.id} rule={rule} onChanged={onChanged} />
      ))}
    </div>
  );
}

/**
 * One series rule.
 *
 * Deleting it asks first, for the same reason deleting a recording does: the control that
 * does it sits beside Pause, which is the reversible version of the same intent, and a
 * rule carries settings — its channel, its weekdays, its time window, how many episodes
 * to keep — that only exist here. Pausing is one click, because pausing is undoable.
 */
function RuleRow({
  rule, onChanged,
}: {
  rule: RecordingRule;
  onChanged: () => void;
}) {
  const [confirming, setConfirming] = useState(false);
  return (
        <RowShell accent={confirming ? 'var(--danger)' : undefined} testId="rule-row">
          <div style={{ minWidth: 0, flex: 1, opacity: rule.enabled ? 1 : 0.55 }}>
            <div style={{ fontWeight: 700 }}>{rule.title}</div>
            <div style={{ fontSize: 'var(--fs-sm)', color: 'var(--text-muted)' }}>
              {rule.channelId ? (rule.channelName ?? 'One channel') : 'Any channel'}
              {rule.newOnly && ' · New episodes only'}
              {rule.weekdays && ` · ${rule.weekdays.map((d) => WEEKDAYS[d] ?? '?').join(', ')}`}
              {rule.aroundLocalMinute != null && ` · around ${
                String(Math.floor(rule.aroundLocalMinute / 60)).padStart(2, '0')
              }:${String(rule.aroundLocalMinute % 60).padStart(2, '0')}`}
              {rule.keepEpisodes != null && ` · keep ${rule.keepEpisodes}`}
            </div>
          </div>

          {confirming ? (
            <>
              <span style={{ fontSize: 'var(--fs-sm)', color: 'var(--danger)', whiteSpace: 'nowrap' }}>
                Stop recording {rule.title} and forget this rule?
              </span>
              <Button
                size="sm"
                variant="danger"
                onClick={async () => {
                  void guard(
                    () => invoke('dvr.deleteRule', { id: rule.id }),
                    'Could not delete that rule',
                    onChanged,
                  );
                }}
              >
                Delete rule
              </Button>
              <Button size="sm" onClick={() => setConfirming(false)}>Keep</Button>
            </>
          ) : (
            <>
              {/*
                * "tracked", not "recorded". `scheduled` counts rows in any state --
                * `SELECT COUNT(*) FROM recordings WHERE rule_id = r.id`, with no
                * `WHERE state`, which the Rust field's own doc comment says ("scheduled
                * or made"). `RecordingState` has five values, so a rule for a show on a
                * channel that keeps dying reported "5 recorded" with five failures and
                * nothing on disk -- and R-108 makes `skipped` the expected outcome for a
                * second concurrent recording on this subscription, so that is not an
                * exotic case (R-119).
                */}
              <span style={{ fontSize: 'var(--fs-sm)', color: 'var(--text-faint)' }}>
                {rule.scheduled} tracked
              </span>
              <Button
                size="sm"
                variant="ghost"
                onClick={async () => {
                  void guard(
                    () =>
                      invoke('dvr.setRuleEnabled', { id: rule.id, value: !rule.enabled }),
                    rule.enabled ? 'Could not pause that rule' : 'Could not resume it',
                    onChanged,
                  );
                }}
              >
                {rule.enabled ? 'Pause' : 'Resume'}
              </Button>
              <IconButton
                size={32}
                icon="close"
                label={`Delete the rule for ${rule.title}`}
                onClick={() => setConfirming(true)}
              />
            </>
          )}
        </RowShell>
  );
}

/* ── Styles ───────────────────────────────────────────────────────────────── */

const headerRow: React.CSSProperties = {
  display: 'flex', alignItems: 'center', gap: 20,
  marginBottom: 'var(--sp-4)', flexWrap: 'wrap',
};

const listStyle: React.CSSProperties = { display: 'grid', gap: 10 };

/** Daylight's recorded library: four cards across. */
const cardGrid: React.CSSProperties = {
  display: 'grid', gap: 16,
  gridTemplateColumns: 'repeat(auto-fill, minmax(300px, 1fr))',
};

/** A chip on artwork: paper on colour. */
const ON_ART_CHIP: React.CSSProperties = {
  background: 'var(--on-art)', color: 'var(--text-on-light)',
};

const sectionHeading: React.CSSProperties = {
  fontFamily: 'var(--font-display)', fontWeight: 800,
  letterSpacing: 'var(--tracking-display)', fontSize: 'var(--fs-2xl)',
  margin: 'var(--sp-6) 0 var(--sp-3)',
};

const noticeStyle: React.CSSProperties = {
  display: 'flex', gap: 'var(--sp-3)', alignItems: 'flex-start',
  padding: 'var(--sp-3)', marginBottom: 'var(--sp-4)',
  borderRadius: 'var(--r-md)',
  background: 'color-mix(in srgb, var(--warning) 16%, var(--surface))',
  border: '1px solid color-mix(in srgb, var(--warning) 45%, transparent)',
  fontWeight: 600,
  fontSize: 'var(--fs-sm)',
};
