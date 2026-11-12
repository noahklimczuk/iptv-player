/** A working slice of README §15 — enough to prove the settings architecture. */
import { useState } from 'react';
import type { Theme } from '@/state/ui';
import { Badge, Button } from '@/components/Primitives';
import { useCommand } from '@/hooks/useCommand';
import { isNativeHost } from '@/ipc';
import { AboutPanel } from '@/features/settings/AboutPanel';
import { DiagnosticsPanel } from '@/features/settings/DiagnosticsPanel';
import { FilterPanel } from '@/features/settings/FilterPanel';
import { GeminiPanel } from '@/features/settings/GeminiPanel';
import { MetadataPanel } from '@/features/settings/MetadataPanel';
import { ProviderEditor } from '@/features/settings/ProviderEditor';
import { RefreshButton } from '@/features/settings/RefreshButton';
import { TimeshiftPanel } from '@/features/settings/TimeshiftPanel';
import { UpdatePanel } from '@/features/settings/UpdatePanel';
import { useUi } from '@/state/ui';

export function SettingsPage({ onAddProvider }: { onAddProvider: () => void }) {
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

  return (
    <div
      style={{
        padding: '4px 36px var(--sp-6)',
        display: 'grid',
        gridTemplateColumns: '240px minmax(0, 1fr)',
        gap: 24,
        alignItems: 'start',
        maxWidth: 1240,
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
            style={{
              display: 'block', padding: '9px 14px', borderRadius: 'var(--r-md)',
              textDecoration: 'none', fontWeight: 700, fontSize: 'var(--fs-md)',
              background: current === title ? 'var(--text)' : 'transparent',
              color: current === title ? 'var(--bg)' : 'var(--text)',
            }}
          >
            {title}
          </a>
        ))}
      </nav>

      <div className="dl-panel" style={{ padding: '24px 28px', minWidth: 0 }}>

      {!isNativeHost() && (
        <div
          style={{
            marginBottom: 'var(--sp-5)', padding: '12px 16px',
            border: '1px solid color-mix(in srgb, var(--warning) 50%, transparent)',
            borderRadius: 'var(--r-md)',
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

      <Section title="Artwork and metadata">
        <MetadataPanel />
      </Section>

      <Section title="Recommendations">
        <GeminiPanel />
      </Section>

      <Section title="Filtering">
        <FilterPanel />
      </Section>

      <Section title="Pause live TV">
        <TimeshiftPanel />
      </Section>

      <Section title="Library">
        {stats && (
          <dl style={{ margin: 0, display: 'grid', gridTemplateColumns: '220px 1fr', gap: 8 }}>
            <dt style={dtStyle}>Channels</dt><dd style={ddStyle}>{stats.channels}</dd>
            <dt style={dtStyle}>Movies</dt><dd style={ddStyle}>{stats.movies}</dd>
            <dt style={dtStyle}>Series</dt><dd style={ddStyle}>{stats.series}</dd>
            <dt style={dtStyle}>Episodes</dt><dd style={ddStyle}>{stats.episodes}</dd>
            <dt style={dtStyle}>EPG coverage</dt>
            <dd style={ddStyle}>
              {stats.epgCoverage.matched} of {stats.epgCoverage.total} channels
              {' '}({Math.round((stats.epgCoverage.matched / stats.epgCoverage.total) * 100)}%)
              {stats.epgCoverage.unmatched.length > 0 && (
                <div style={{ color: 'var(--text-faint)', fontSize: 'var(--fs-sm)' }}>
                  Unmatched: {stats.epgCoverage.unmatched.join(', ')}
                </div>
              )}
            </dd>
          </dl>
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
        <dl style={{ margin: 0, display: 'grid', gridTemplateColumns: '220px 1fr', gap: 8 }}>
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
            <div key={k} style={{ display: 'contents' }}>
              <dt style={dtStyle}><kbd style={kbdStyle}>{k}</kbd></dt>
              <dd style={ddStyle}>{v}</dd>
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
  'Providers', 'Appearance', 'Artwork and metadata', 'Recommendations', 'Filtering',
  'Pause live TV', 'Library', 'Updates', 'About', 'Diagnostics', 'Keyboard',
];

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
      aria-labelledby={headingId}
      style={{ marginBottom: 'var(--sp-6)', scrollMarginTop: 'calc(var(--topbar-h) + 8px)' }}
    >
      <h2
        id={headingId}
        style={{
          margin: '0 0 var(--sp-2)', fontSize: 'var(--fs-sm)', fontWeight: 800,
          color: 'var(--text-muted)',
        }}
      >
        {title}
      </h2>
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
    <div
      style={{
        display: 'flex', alignItems: 'center', gap: 'var(--sp-4)',
        padding: '16px 0', borderBottom: '1px solid var(--border)',
      }}
    >
      <div style={{ flex: 1 }}>
        <div style={{ fontWeight: 800, fontSize: 'var(--fs-lg)' }}>{label}</div>
        {hint && (
          <div style={{ fontSize: 'var(--fs-sm)', color: 'var(--text-muted)' }}>{hint}</div>
        )}
      </div>
      {children}
    </div>
  );
}

const rowStyle = {
  display: 'flex', alignItems: 'center', justifyContent: 'space-between',
  gap: 'var(--sp-4)', padding: 'var(--sp-2) 0', flexWrap: 'wrap',
} as const;
/** A provider is its own card inside the panel, as on the board. */
const providerCard = {
  padding: '10px 18px', marginBottom: 10, borderRadius: 'var(--r-lg)',
  border: '1px solid var(--border)', boxShadow: 'var(--shadow-1)',
} as const;
const dtStyle = { color: 'var(--text-faint)' } as const;
const ddStyle = { margin: 0, color: 'var(--text-muted)' } as const;
const kbdStyle = {
  background: 'var(--surface-sunk)', borderRadius: 6, padding: '2px 8px',
  fontSize: 'var(--fs-xs)', fontWeight: 800, fontFamily: 'inherit',
} as const;
