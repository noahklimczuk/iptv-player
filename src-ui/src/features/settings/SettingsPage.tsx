/** A working slice of README §15 — enough to prove the settings architecture. */
import type { Theme } from '@/state/ui';
import { Badge, Button } from '@/components/Primitives';
import { useCommand } from '@/hooks/useCommand';
import { isNativeHost } from '@/ipc';
import { FilterPanel } from '@/features/settings/FilterPanel';
import { MetadataPanel } from '@/features/settings/MetadataPanel';
import { UpdatePanel } from '@/features/settings/UpdatePanel';
import { useUi } from '@/state/ui';

export function SettingsPage({ onAddProvider }: { onAddProvider: () => void }) {
  const ui = useUi();
  const { data: providers } = useCommand('providers.list', undefined, []);
  const { data: stats } = useCommand('library.stats', undefined, []);

  return (
    <div style={{ padding: 'var(--sp-5) var(--sp-6)', maxWidth: 860 }}>
      <h1 style={{ margin: '0 0 var(--sp-5)', fontSize: 'var(--fs-2xl)', fontWeight: 800 }}>
        Settings
      </h1>

      {!isNativeHost() && (
        <div
          style={{
            marginBottom: 'var(--sp-5)', padding: 'var(--sp-3) var(--sp-4)',
            border: '1px solid var(--warning)', borderRadius: 'var(--r-md)',
            background: 'color-mix(in srgb, var(--warning) 12%, transparent)',
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
            <div key={p.id} style={rowStyle}>
              <div>
                <div style={{ fontWeight: 650 }}>{p.name}</div>
                <div style={{ fontSize: 'var(--fs-sm)', color: 'var(--text-muted)' }}>
                  {p.kind.toUpperCase()} · {p.channelCount} channels ·
                  {' '}{p.movieCount} movies · {p.seriesCount} series
                </div>
              </div>
              <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
                {p.maxConnections != null && (
                  <Badge tone="outline">
                    {p.activeConnections ?? 0}/{p.maxConnections} connections
                  </Badge>
                )}
                {days != null && (
                  <Badge tone={days < 7 ? 'live' : 'neutral'}>
                    {days < 0 ? 'Expired' : `${days} days left`}
                  </Badge>
                )}
              </div>
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
        <Field label="Theme">
          <div style={{ display: 'flex', gap: 6 }}>
            {(['dark', 'oled', 'light', 'contrast'] as Theme[]).map((t) => (
              <Button
                key={t} size="sm"
                variant={ui.theme === t ? 'primary' : 'secondary'}
                onClick={() => ui.setTheme(t)}
                style={{ textTransform: 'capitalize' }}
              >
                {t}
              </Button>
            ))}
          </div>
        </Field>
        <Field label="Interface density" hint="TV mode enlarges everything for a 10-foot UI.">
          <div style={{ display: 'flex', gap: 6 }}>
            <Button
              size="sm" variant={ui.density === 'desktop' ? 'primary' : 'secondary'}
              onClick={() => ui.setDensity('desktop')}
            >
              Desktop
            </Button>
            <Button
              size="sm" variant={ui.density === 'tv' ? 'primary' : 'secondary'}
              onClick={() => ui.setDensity('tv')}
            >
              TV
            </Button>
          </div>
        </Field>
        <Field label="Animations" hint="Disables hover previews, trailer autoplay and transitions.">
          <Button size="sm" onClick={ui.toggleAnimations}>
            {ui.animations ? 'On' : 'Off'}
          </Button>
        </Field>
        <Field label="Hover previews">
          <Button size="sm" onClick={ui.toggleHoverPreviews}>
            {ui.hoverPreviews ? 'On' : 'Off'}
          </Button>
        </Field>
      </Section>

      <Section title="Artwork and metadata">
        <MetadataPanel />
      </Section>

      <Section title="Filtering">
        <FilterPanel />
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

      <Section title="Keyboard">
        <dl style={{ margin: 0, display: 'grid', gridTemplateColumns: '220px 1fr', gap: 8 }}>
          {[
            ['Ctrl+K  /', 'Search and command palette'],
            ['0–9', 'Direct channel entry'],
            ['Backspace', 'Last channel'],
            ['PgUp / PgDn', 'Channel up / down'],
            ['G', 'TV guide'],
            ['Space / K', 'Play-pause'],
            ['← / →', 'Seek ±10s (Shift: ±30s)'],
            ['↑ / ↓', 'Volume · M mute'],
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
  );
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section style={{ marginBottom: 'var(--sp-6)' }}>
      <h2
        style={{
          margin: '0 0 var(--sp-3)', fontSize: 'var(--fs-xs)', fontWeight: 700,
          letterSpacing: '0.08em', textTransform: 'uppercase', color: 'var(--text-faint)',
        }}
      >
        {title}
      </h2>
      <div
        style={{
          background: 'var(--bg-elevated)', border: '1px solid var(--border)',
          borderRadius: 'var(--r-lg)', padding: 'var(--sp-4)',
        }}
      >
        {children}
      </div>
    </section>
  );
}

function Field({
  label, hint, children,
}: { label: string; hint?: string; children: React.ReactNode }) {
  return (
    <div
      style={{
        display: 'flex', alignItems: 'center', gap: 'var(--sp-4)',
        padding: 'var(--sp-2) 0',
      }}
    >
      <div style={{ flex: 1 }}>
        <div style={{ fontWeight: 600 }}>{label}</div>
        {hint && (
          <div style={{ fontSize: 'var(--fs-sm)', color: 'var(--text-faint)' }}>{hint}</div>
        )}
      </div>
      {children}
    </div>
  );
}

const rowStyle = {
  display: 'flex', alignItems: 'center', justifyContent: 'space-between',
  gap: 'var(--sp-4)', padding: 'var(--sp-2) 0',
} as const;
const dtStyle = { color: 'var(--text-faint)' } as const;
const ddStyle = { margin: 0, color: 'var(--text-muted)' } as const;
const kbdStyle = {
  border: '1px solid var(--border-strong)', borderRadius: 4, padding: '1px 6px',
  fontSize: 'var(--fs-xs)', fontFamily: 'ui-monospace, monospace',
} as const;
