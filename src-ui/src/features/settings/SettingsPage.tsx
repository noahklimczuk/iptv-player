/** A working slice of README §15 — enough to prove the settings architecture. */
import { useEffect, useState } from 'react';
import { useSearchParams } from 'react-router-dom';
import type { Theme } from '@/state/ui';
import { Badge, Button } from '@/components/Primitives';
import { Icon, type IconName } from '@/components/Icon';
import { useCommand } from '@/hooks/useCommand';
import { invoke, isNativeHost, requestParentalPin } from '@/ipc';
import { AboutPanel } from '@/features/settings/AboutPanel';
import { DiagnosticsPanel } from '@/features/settings/DiagnosticsPanel';
import { FilterPanel } from '@/features/settings/FilterPanel';
import { GeminiPanel } from '@/features/settings/GeminiPanel';
import { MetadataPanel } from '@/features/settings/MetadataPanel';
import { ParentalPanel } from '@/features/settings/ParentalPanel';
import { PrivacyPanel } from '@/features/settings/PrivacyPanel';
import { SyncPanel } from '@/features/settings/SyncPanel';
import { ProviderEditor } from '@/features/settings/ProviderEditor';
import { RefreshButton } from '@/features/settings/RefreshButton';
import { TimeshiftPanel } from '@/features/settings/TimeshiftPanel';
import { UpdatePanel } from '@/features/settings/UpdatePanel';
import { useParental } from '@/state/parental';
import { useProfile } from '@/state/profile';
import { useUi } from '@/state/ui';

/**
 * Settings, behind the parental PIN when the household asked for that or a kids profile
 * is watching (README §11: "a PIN-protected Settings section", and a Kids profile
 * "disables settings access").
 *
 * Leaving closes what the PIN opened, so the next person to come here is asked again
 * rather than inheriting the parent's half hour.
 */
export function SettingsPage({ onAddProvider }: { onAddProvider: () => void }) {
  const parental = useProfile((s) => s.parental);
  const state = useParental((s) => s.state);
  const loadParental = useParental((s) => s.load);
  const guarded = parental?.hasMasterPin ?? false;

  useEffect(() => {
    loadParental().catch(() => {});
  }, [loadParental]);
  useEffect(() => () => {
    if (guarded) invoke('parental.relock').catch(() => {});
  }, [guarded]);

  // The host's answer (`settingsLocked`): a kids profile is kept out as well as everybody
  // when the household locked Settings, and the host refuses the commands behind this
  // page while it holds, so this screen is a courtesy rather than the lock. Until the
  // state arrives, the household switch is the best guess, and the closed one.
  const locked = state ? state.settingsLocked : Boolean(parental?.lockSettings && guarded);
  if (locked) {
    return (
      <div style={{ display: 'grid', placeItems: 'center', padding: 'var(--sp-7) var(--sp-5)' }}>
        <div className="dl-panel settings-card" style={{ maxWidth: 520, textAlign: 'center' }}>
          <Icon name="lock" size={32} />
          <h1 className="dl-display" style={{ fontSize: 'var(--fs-2xl)', margin: '12px 0 8px' }}>
            Settings are locked
          </h1>
          <p className="settings-row-hint" style={{ margin: '0 auto 20px' }}>
            Enter the parental PIN to change them.
          </p>
          <Button
            variant="primary"
            icon="lock"
            onClick={() => {
              void requestParentalPin('change').then(async (ok) => { if (ok) await loadParental(); });
            }}
          >
            Enter the PIN
          </Button>
        </div>
      </div>
    );
  }
  return <SettingsContent onAddProvider={onAddProvider} />;
}

function SettingsContent({ onAddProvider }: { onAddProvider: () => void }) {
  /**
   * The eight fields this page reads, subscribed to one at a time (R-90).
   *
   * `useUi()` with no selector subscribes to the whole store, including `player`, which
   * the host's heartbeat writes four times a second. Settings reads none of it -- theme,
   * density and two toggles -- and re-rendered the entire page, every panel in it, at
   * 4 Hz whenever something was playing in the background.
   *
   * Kept as an object so the reads below are unchanged, which is the shape `App` already
   * uses: what matters is what is subscribed to, not how it is spelled at the call site.
   */
  const ui = {
    theme: useUi((s) => s.theme),
    density: useUi((s) => s.density),
    animations: useUi((s) => s.animations),
    hoverPreviews: useUi((s) => s.hoverPreviews),
    // `use24h` and `toggle24h` arrived on a different branch from the conversion above,
    // and git merged both without a conflict because they are different hunks of the
    // same file: the 24-hour switch below reads fields this list did not name, and the
    // page stopped compiling the moment the two were in one tree. Named here, which is
    // the whole point of the list -- the previous `useUi()` would have carried them
    // silently and re-rendered this page on every player heartbeat to do it.
    use24h: useUi((s) => s.use24h),
    glassClarity: useUi((s) => s.glassClarity),
    setGlassClarity: useUi((s) => s.setGlassClarity),
    setTheme: useUi((s) => s.setTheme),
    setDensity: useUi((s) => s.setDensity),
    toggleAnimations: useUi((s) => s.toggleAnimations),
    toggleHoverPreviews: useUi((s) => s.toggleHoverPreviews),
    toggle24h: useUi((s) => s.toggle24h),
  };
  const { data: providers, reload: reloadProviders } = useCommand('providers.list', undefined, []);
  const [editing, setEditing] = useState<number | null>(null);
  const { data: stats } = useCommand('library.stats', undefined, []);
  const [current, setCurrent] = useState(SECTIONS[0]!);
  useCurrentSection(setCurrent);

  // `#/settings?section=Account and sync` opens the page at that section -- how the
  // setup wizard hands someone over to creating an account.
  const [params] = useSearchParams();
  const jump = params.get('section');
  useEffect(() => {
    const title = SECTIONS.find((t) => t === jump);
    if (!title) return undefined;
    setCurrent(title);
    // After the cards above it have laid out, or it lands short.
    const id = window.setTimeout(
      () => document.getElementById(sectionId(title))?.scrollIntoView({ block: 'start' }),
      80,
    );
    return () => window.clearTimeout(id);
  }, [jump]);

  return (
    <div
      style={{
        padding: '4px 36px var(--sp-6)',
        display: 'grid',
        gridTemplateColumns: '248px minmax(0, 1fr)',
        gap: 28,
        alignItems: 'start',
        maxWidth: 1180,
        // Centred in the space the page has, like the single column it replaces.
        margin: '0 auto',
        position: 'relative',
      }}
    >
      {/* Named for assistive technology; the top bar already says Settings on screen. */}
      <h1 className="sr-only">Settings</h1>
      {/* Daylight's section index: a white panel, the current section on an ink pill.
          Links that scroll the page, not routes: the sections are all on this one. */}
      <nav
        aria-label="Settings sections"
        className="dl-panel"
        style={{
          position: 'sticky', top: 'calc(var(--topbar-h) + 4px)', padding: 12,
          display: 'grid', gap: 2,
        }}
      >
        {SECTIONS.map((title) => (
          <a
            key={title}
            href={`#${sectionId(title)}`}
            aria-current={current === title ? 'true' : undefined}
            onClick={(e) => {
              // Not a navigation: the router owns the hash.
              e.preventDefault();
              setCurrent(title);
              document.getElementById(sectionId(title))
                ?.scrollIntoView({ behavior: 'smooth', block: 'start' });
            }}
            className="settings-nav-link"
          >
            <Icon name={SECTION_ICONS[title] ?? 'settings'} size={17} />
            {title}
          </a>
        ))}
      </nav>

      <div style={{ display: 'grid', gap: 20, minWidth: 0 }}>

      {!isNativeHost() && (
        <div
          style={{
            padding: '14px 20px',
            border: '1px solid color-mix(in srgb, var(--warning) 50%, transparent)',
            borderRadius: 'var(--r-xl)',
            background: 'color-mix(in srgb, var(--warning) 14%, var(--surface))',
            fontSize: 'var(--fs-sm)',
          }}
        >
          <strong>Browser preview.</strong> Running without the Tauri host, so data comes
          from the in-memory mock and playback is simulated. On Windows the same UI talks
          to libmpv and SQLite.
        </div>
      )}

      <Section title="Providers">
        {(providers ?? []).map((p) => {
          const days = p.expiresAt
            ? Math.round((p.expiresAt - Date.now() / 1000) / 86400)
            : null;
          return (
            <div key={p.id} style={providerCard}>
            <div style={rowStyle}>
              <div>
                <div style={{ fontWeight: 800, fontSize: 'var(--fs-lg)' }}>{p.name}</div>
                <div style={{ fontSize: 'var(--fs-sm)', color: 'var(--text-muted)' }}>
                  {p.kind.toUpperCase()} · {p.channelCount} channels ·
                  {' '}{p.movieCount} movies · {p.seriesCount} series
                </div>
              </div>
              <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
                {p.maxConnections != null && (
                  <Badge tone="neutral">
                    {p.activeConnections ?? 0}/{p.maxConnections} connections
                  </Badge>
                )}
                {days != null && (
                  <Badge tone={days < 7 ? 'live' : 'new'}>
                    {days < 0 ? 'Expired' : `${days} days left`}
                  </Badge>
                )}
                <RefreshButton providerId={p.id} onDone={reloadProviders} />
                <Button
                  size="sm"
                  aria-expanded={editing === p.id}
                  onClick={() => setEditing(editing === p.id ? null : p.id)}
                >
                  {editing === p.id ? 'Close' : 'Edit'}
                </Button>
              </div>
            </div>
            {editing === p.id && (
              <ProviderEditor
                provider={p}
                onClose={() => setEditing(null)}
                onChanged={reloadProviders}
              />
            )}
            </div>
          );
        })}
        <Button
          size="sm"
          icon="plus"
          style={{ marginTop: 'var(--sp-3)' }}
          onClick={onAddProvider}
        >
          Add provider
        </Button>
      </Section>

      <Section title="Appearance">
        <Field label="Theme" hint="Applies to every screen except the player.">
          <div className="dl-seg">
            {(['dark', 'oled', 'light', 'contrast'] as Theme[]).map((t) => (
              <button
                key={t}
                type="button"
                data-active={ui.theme === t}
                data-variant={ui.theme === t ? 'primary' : 'secondary'}
                onClick={() => ui.setTheme(t)}
                style={{ textTransform: 'capitalize' }}
              >
                {t}
              </button>
            ))}
          </div>
        </Field>
        <Field label="Interface density" hint="TV mode enlarges everything for a 10-foot UI.">
          <div className="dl-seg">
            <button
              type="button"
              data-active={ui.density === 'desktop'}
              data-variant={ui.density === 'desktop' ? 'primary' : 'secondary'}
              onClick={() => ui.setDensity('desktop')}
            >
              Desktop
            </button>
            <button
              type="button"
              data-active={ui.density === 'tv'}
              data-variant={ui.density === 'tv' ? 'primary' : 'secondary'}
              onClick={() => ui.setDensity('tv')}
            >
              TV
            </button>
          </div>
        </Field>
        <Field
          label="Glass transparency"
          hint="How see-through the tab bar, menus and player controls are. Lower is easier to read over busy pictures."
        >
          <div style={{ display: 'flex', alignItems: 'center', gap: 'var(--sp-3)' }}>
            <span style={{ fontSize: 'var(--fs-xs)', color: 'var(--text-faint)', fontWeight: 700 }}>
              Frosted
            </span>
            <input
              type="range"
              min={0}
              max={100}
              step={5}
              value={ui.glassClarity}
              aria-label="Glass transparency"
              aria-valuetext={`${ui.glassClarity}%`}
              data-testid="glass-clarity"
              onChange={(e) => ui.setGlassClarity(Number(e.target.value))}
              style={{ flex: 1, minWidth: 160, maxWidth: 260, accentColor: 'var(--accent)' }}
            />
            <span style={{ fontSize: 'var(--fs-xs)', color: 'var(--text-faint)', fontWeight: 700 }}>
              Clear
            </span>
            <span
              style={{ fontVariantNumeric: 'tabular-nums', fontWeight: 700, minWidth: '4ch' }}
            >
              {ui.glassClarity}%
            </span>
          </div>
        </Field>
        <Field label="Animations" hint="Disables hover previews, trailer autoplay and transitions.">
          <Toggle label="Animations" on={ui.animations} onToggle={ui.toggleAnimations} />
        </Field>
        <Field label="Hover previews" hint="Plays a title's trailer when a card expands.">
          <Toggle
            label="Hover previews"
            on={ui.hoverPreviews}
            onToggle={ui.toggleHoverPreviews}
          />
        </Field>
        {/*
          * The control the preference never had. `use24h` was in the store and honoured
          * by `clockTime`, and nothing set it or read it -- so the AM/PM branch was
          * unreachable code behind a setting with no switch (R-136).
          */}
        <Field label="24-hour clock" hint="Off shows times as 9:30 PM rather than 21:30.">
          <Toggle label="24-hour clock" on={ui.use24h} onToggle={ui.toggle24h} />
        </Field>
      </Section>

      <Section title="Account and sync">
        <SyncPanel />
      </Section>

      <Section title="Parental controls">
        <ParentalPanel />
      </Section>

      <Section title="Artwork and metadata">
        <MetadataPanel />
      </Section>

      <Section title="Recommendations">
        <GeminiPanel />
      </Section>

      <Section title="Privacy">
        <PrivacyPanel />
      </Section>

      <Section title="Filtering">
        <FilterPanel />
      </Section>

      <Section title="Pause live TV">
        <TimeshiftPanel />
      </Section>

      <Section title="Library">
        {stats && (
          <>
            <dl className="settings-tiles">
              {([
                ['Channels', stats.channels],
                ['Movies', stats.movies],
                ['Series', stats.series],
                ['Episodes', stats.episodes],
              ] as const).map(([label, value]) => (
                <div key={label} className="settings-tile">
                  <dt>{label}</dt>
                  <dd className="dl-display">{value.toLocaleString()}</dd>
                </div>
              ))}
            </dl>
            <div className="settings-row" style={{ display: 'block' }}>
              <div style={{ display: 'flex', justifyContent: 'space-between', gap: 16 }}>
                <span className="settings-row-label">Guide coverage</span>
                <span style={{ fontWeight: 700, color: 'var(--text-muted)' }}>
                  {stats.epgCoverage.matched} of {stats.epgCoverage.total} channels
                  {' '}({epgPercent(stats.epgCoverage)}%)
                </span>
              </div>
              <div className="settings-meter" aria-hidden>
                <div style={{ width: `${epgPercent(stats.epgCoverage)}%` }} />
              </div>
              {stats.epgCoverage.unmatched.length > 0 && (
                <div className="settings-row-hint">
                  No guide for: {stats.epgCoverage.unmatched.join(', ')}
                </div>
              )}
            </div>
          </>
        )}
      </Section>

      <Section title="Updates">
        <UpdatePanel />
      </Section>

      <Section title="About">
        <AboutPanel />
      </Section>

      <Section title="Diagnostics">
        <DiagnosticsPanel />
      </Section>

      <Section title="Keyboard">
        {/*
          * Every binding `useHotkeys` interprets, which is what this list is for.
          *
          * It showed ten rows against eighteen bindings. The six missing ones were
          * `Escape`, `J`, `L`, `F`, `P` and `I` — and two of those had no documentation
          * anywhere else at all: `I` reaches the channel banner and the detail panel,
          * and `P` reaches 832 lines of picture-in-picture whose only discoverable
          * trace was the source. README §14.1 promises a sheet that `?` opens and a
          * `docs/SHORTCUTS.md`; neither exists, so this panel is the sheet (R-143).
          */}
        <dl className="settings-keys">
          {[
            ['Ctrl+K  /', 'Search and command palette'],
            ['Escape', 'Back · close a dialog'],
            ['0–9', 'Direct channel entry'],
            ['Backspace', 'Last channel'],
            ['PgUp / PgDn', 'Channel up / down'],
            ['G', 'TV guide'],
            ['Space / K', 'Play-pause'],
            ['← / →', 'Seek ±10s (Shift: ±30s)'],
            ['J / L', 'Seek ±10s'],
            ['T', 'Pause live TV · press again for the live edge'],
            ['↑ / ↓', 'Volume · M mute'],
            ['Mouse wheel', 'Volume, scrolling over the picture'],
            ['F / F11', 'Fullscreen'],
            ['P', 'Picture-in-picture'],
            ['I', 'Info · the channel banner, or a title’s details'],
            ['Alt+1…5', 'Home, Live, Guide, Movies, Series'],
          ].map(([k, v]) => (
            <div key={k}>
              <dt><kbd>{k}</kbd></dt>
              <dd>{v}</dd>
            </div>
          ))}
        </dl>
      </Section>
      </div>
    </div>
  );
}

/** The sections, in page order, for the index. */
const SECTIONS = [
  'Providers', 'Appearance', 'Account and sync', 'Parental controls', 'Artwork and metadata',
  'Recommendations',
  'Privacy', 'Filtering',
  'Pause live TV', 'Library', 'Updates', 'About', 'Diagnostics', 'Keyboard',
];

/** The side index's icons, matching the bottom bar's line-icon style. */
const SECTION_ICONS: Record<string, IconName> = {
  Providers: 'layers',
  Appearance: 'sun',
  'Account and sync': 'person',
  'Parental controls': 'lock',
  'Artwork and metadata': 'film',
  Recommendations: 'sparkle',
  Privacy: 'clock',
  Filtering: 'grid',
  'Pause live TV': 'pause',
  Library: 'stack',
  Updates: 'refresh',
  About: 'info',
  Diagnostics: 'alert',
  Keyboard: 'keyboard',
};

/**
 * One line under each section's heading, saying what it is for. Absent where the
 * section's own first paragraph already says it.
 */
const SECTION_BLURBS: Record<string, string> = {
  Providers: 'The playlists and Xtream lines your library is built from.',
  Appearance: 'How Aurora looks and moves.',
  'Account and sync': 'Your Aurora account, and keeping every device in step.',
  'Parental controls': 'What each profile may watch, and what needs the PIN.',
  Privacy: 'What Aurora remembers you watched, and for how long.',
  Filtering: 'What your library shows, and what it hides.',
  'Pause live TV': 'Rewind and pause the channel you are watching.',
  Library: 'What your providers have given you.',
  Updates: 'New versions of Aurora.',
  About: 'This copy of Aurora, and the work it is built on.',
  Diagnostics: 'For when something is not working.',
  Keyboard: 'Every shortcut, in one place.',
};

function epgPercent(c: { matched: number; total: number }): number {
  return c.total > 0 ? Math.round((c.matched / c.total) * 100) : 0;
}

/**
 * Keep the side index on the section being read.
 *
 * It only moved when clicked, so after scrolling it went on claiming "Providers" while
 * the Keyboard section was on screen. Watches which section crosses a line a third of
 * the way down the scroller -- the one somebody is reading -- and names that one.
 */
function useCurrentSection(setCurrent: (title: string) => void) {
  useEffect(() => {
    const root = document.querySelector<HTMLElement>('[data-scroll-root]');
    if (typeof IntersectionObserver === 'undefined') return;
    const observer = new IntersectionObserver(
      (entries) => {
        const hit = entries.find((e) => e.isIntersecting);
        const title = hit && SECTIONS.find((t) => `${sectionId(t)}-card` === hit.target.id);
        if (title) setCurrent(title);
      },
      { root, rootMargin: '-30% 0px -65% 0px' },
    );
    for (const title of SECTIONS) {
      const el = document.getElementById(`${sectionId(title)}-card`);
      if (el) observer.observe(el);
    }
    return () => observer.disconnect();
  }, [setCurrent]);
}

function sectionId(title: string): string {
  return `settings-${title.toLowerCase().replace(/[^a-z0-9]+/g, '-')}`;
}

/**
 * One titled card of settings.
 *
 * The heading names the section to assistive technology as well as to the eye, which
 * turns each card into its own addressable region. That is worth the two extra lines: a
 * page of eight panels is otherwise one undifferentiated blob to a screen reader, and
 * anything looking for a control inside one panel has to search the whole page. The
 * second half of that bit us — a field labelled "Gemini API key" and one labelled
 * "API key" are the same field to a substring match, so adding the first broke every
 * journey that went looking for the second.
 */
function Section({ title, children }: { title: string; children: React.ReactNode }) {
  const headingId = sectionId(title);
  return (
    <section
      id={`${headingId}-card`}
      aria-labelledby={headingId}
      className="dl-panel settings-card"
    >
      <header className="settings-card-head">
        <h2 id={headingId} className="dl-display">{title}</h2>
        {SECTION_BLURBS[title] && <p>{SECTION_BLURBS[title]}</p>}
      </header>
      <div>{children}</div>
    </section>
  );
}

/**
 * An on/off control that says what it is for.
 *
 * These were bare buttons reading "On" — which is the whole of their accessible name, so
 * a screen reader announced "On, button" twice in a row with nothing to distinguish the
 * two, and neither carried the state as state. `role="switch"` with `aria-checked` is
 * what the control actually is; the `Field` beside it supplies the visible label and this
 * repeats it for anything that cannot see the layout.
 */
function Toggle({
  label, on, onToggle,
}: { label: string; on: boolean; onToggle: () => void }) {
  return (
    <button
      type="button"
      className="dl-switch"
      role="switch"
      aria-checked={on}
      aria-label={label}
      title={on ? 'On' : 'Off'}
      onClick={onToggle}
    />
  );
}

function Field({
  label, hint, children,
}: { label: string; hint?: string; children: React.ReactNode }) {
  return (
    <div className="settings-row">
      <div style={{ flex: 1, minWidth: 0 }}>
        <div className="settings-row-label">{label}</div>
        {hint && <div className="settings-row-hint">{hint}</div>}
      </div>
      {children}
    </div>
  );
}

const rowStyle = {
  display: 'flex', alignItems: 'center', justifyContent: 'space-between',
  gap: 'var(--sp-4)', padding: 'var(--sp-2) 0', flexWrap: 'wrap',
} as const;
/** A provider is a sunk tile inside its section's card. */
const providerCard = {
  padding: '12px 18px', marginBottom: 10, borderRadius: 'var(--r-lg)',
  background: 'var(--surface-sunk)',
} as const;
