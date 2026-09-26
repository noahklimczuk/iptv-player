/**
 * First-run wizard (README §13): add provider → validate live → choose content →
 * import with progress → done. Target is under 60 seconds from nothing to watching.
 *
 * Paste-detection is the first thing it does: drop a full `get.php` URL in and the
 * host splits out host, username and password rather than making you do it.
 */
import { AnimatePresence, motion } from 'framer-motion';
import { useCallback, useEffect, useRef, useState } from 'react';
import type {
  DraftProvider, IngestPhase, IngestProgress, SyncReport, ValidationResult,
} from '@shared/ipc';
import { Badge, Button, EmptyState, FIELD } from '@/components/Primitives';
import { Icon } from '@/components/Icon';
import { invoke, onIngestProgress } from '@/ipc';

type Step = 'source' | 'content' | 'importing' | 'done';

const PHASE_LABEL: Record<IngestPhase, string> = {
  authenticating: 'Checking your subscription',
  fetchingPlaylist: 'Fetching your channel list',
  importingChannels: 'Importing channels',
  importingMovies: 'Importing movies',
  importingSeries: 'Importing series',
  fetchingEpg: 'Downloading the TV guide',
  matchingEpg: 'Matching the guide to your channels',
  indexing: 'Building the search index',
  done: 'Finished',
};

export function SetupWizard({
  onFinished,
  onSkip,
  /** True when this is a second provider being added from settings, not first run. */
  additional = false,
}: {
  onFinished: () => void;
  /**
   * Leave without adding anything.
   *
   * Separate from `onFinished` because the two are not the same event and were being
   * treated as though they were: finishing leaves a provider behind, which is what
   * makes the app open; skipping leaves none, and the caller has to say so or its own
   * "no provider yet" guard puts the wizard straight back.
   */
  onSkip: () => void;
  additional?: boolean;
}) {
  const [step, setStep] = useState<Step>('source');
  const [draft, setDraft] = useState<DraftProvider>({
    name: '', kind: 'm3u', url: '', username: '', password: '',
  });
  const [pasted, setPasted] = useState('');
  const [detected, setDetected] = useState<'none' | 'credentials' | 'panel'>('none');
  const [checking, setChecking] = useState(false);
  const [validation, setValidation] = useState<ValidationResult | null>(null);
  const [content, setContent] = useState({ live: true, vod: true, series: true });
  const [progress, setProgress] = useState<IngestProgress | null>(null);
  const [report, setReport] = useState<SyncReport | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => onIngestProgress(setProgress), []);

  /** Recognise what was pasted and fill the form from it. */
  const detect = useCallback(async (text: string) => {
    setPasted(text);
    setValidation(null);
    if (!text.trim()) { setDetected('none'); return; }
    const d = await invoke('providers.detect', { text });
    // Two different things, and saying the wrong one is a lie the user can see: a URL
    // carrying credentials fills the fields in, a bare panel host only offers them.
    if (d.kind !== 'xtream') setDetected('none');
    else setDetected(d.username ? 'credentials' : 'panel');
    setDraft((prev) => ({
      ...prev,
      kind: d.kind,
      url: d.url,
      username: d.username ?? '',
      password: d.password ?? '',
      name: prev.name || hostOf(d.url),
    }));
  }, []);

  const validate = useCallback(async () => {
    setChecking(true);
    setError(null);
    try {
      setValidation(await invoke('providers.validate', { draft }));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setChecking(false);
    }
  }, [draft]);

  const runImport = useCallback(async () => {
    setStep('importing');
    setError(null);
    try {
      const { id } = await invoke('providers.save', { draft });
      setReport(await invoke('providers.refresh', { providerId: id }));
      setStep('done');
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setStep('content');
    }
  }, [draft]);

  return (
    <div
      style={{
        position: 'fixed', inset: 0, zIndex: 400, background: 'var(--bg)',
        display: 'grid', placeItems: 'center', padding: 'var(--sp-5)',
        overflowY: 'auto',
      }}
    >
      <div style={{ width: 'min(620px, 100%)' }}>
        <Header step={step} additional={additional} />

        <AnimatePresence mode="wait">
          <motion.div
            key={step}
            initial={{ opacity: 0, y: 12 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -12 }}
            transition={{ duration: 0.2 }}
          >
            {step === 'source' && (
              <SourceStep
                draft={draft} setDraft={setDraft}
                pasted={pasted} onPaste={detect} detected={detected}
                checking={checking} validation={validation} onCheck={validate}
                onNext={() => setStep('content')}
              />
            )}
            {step === 'content' && (
              <ContentStep
                content={content} setContent={setContent}
                onBack={() => setStep('source')} onImport={runImport}
              />
            )}
            {step === 'importing' && <ImportingStep progress={progress} />}
            {step === 'done' && <DoneStep report={report} onFinished={onFinished} />}
          </motion.div>
        </AnimatePresence>

        {error && (
          <div
            role="alert"
            style={{
              marginTop: 'var(--sp-4)', padding: 'var(--sp-3) var(--sp-4)',
              border: '1px solid var(--danger)', borderRadius: 'var(--r-md)',
              background: 'color-mix(in srgb, var(--danger) 12%, transparent)',
              fontSize: 'var(--fs-sm)',
            }}
          >
            {error}
          </div>
        )}

        <div style={{ marginTop: 'var(--sp-5)', textAlign: 'center' }}>
          <button
            onClick={onSkip}
            style={{
              background: 'none', border: 'none', cursor: 'pointer',
              color: 'var(--text-faint)', fontSize: 'var(--fs-sm)',
            }}
          >
            Skip for now
          </button>
        </div>
      </div>
    </div>
  );
}

function hostOf(url: string): string {
  try {
    return new URL(url).hostname;
  } catch {
    return '';
  }
}

function Header({ step, additional }: { step: Step; additional: boolean }) {
  const steps: Step[] = ['source', 'content', 'importing', 'done'];
  const index = steps.indexOf(step);
  return (
    <div style={{ textAlign: 'center', marginBottom: 'var(--sp-6)' }}>
      <div
        style={{
          width: 52, height: 52, borderRadius: 'var(--r-lg)', margin: '0 auto var(--sp-4)',
          display: 'grid', placeItems: 'center', fontWeight: 900, fontSize: 24, color: '#fff',
          background: 'linear-gradient(135deg, var(--accent), var(--accent-2))',
        }}
      >
        A
      </div>
      <h1 style={{ margin: '0 0 var(--sp-2)', fontSize: 'var(--fs-2xl)', fontWeight: 800 }}>
        {additional ? 'Add another provider' : 'Welcome to Aurora TV'}
      </h1>
      <p style={{ margin: 0, color: 'var(--text-muted)', fontSize: 'var(--fs-sm)' }}>
        {additional
          ? 'Its channels and titles join the library you already have.'
          : 'Aurora is a player. Bring your own subscription and it will do the rest.'}
      </p>
      <div style={{ display: 'flex', gap: 6, justifyContent: 'center', marginTop: 'var(--sp-4)' }}>
        {steps.map((s, i) => (
          <div
            key={s}
            style={{
              width: i === index ? 26 : 8, height: 4, borderRadius: 'var(--r-full)',
              background: i <= index ? 'var(--accent)' : 'var(--border)',
              transition: 'all var(--t-base) var(--ease)',
            }}
          />
        ))}
      </div>
    </div>
  );
}

const card = {
  background: 'var(--bg-elevated)', border: '1px solid var(--border)',
  borderRadius: 'var(--r-lg)', padding: 'var(--sp-5)',
} as const;

/** One field, defined once. See `FIELD` in Primitives. */
const input: React.CSSProperties = { ...FIELD, width: '100%', padding: '11px 14px', fontSize: 'var(--fs-md)' };

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <label style={{ display: 'block', marginBottom: 'var(--sp-3)' }}>
      <span
        style={{
          display: 'block', marginBottom: 5, fontSize: 'var(--fs-sm)',
          fontWeight: 600, color: 'var(--text-muted)',
        }}
      >
        {label}
      </span>
      {children}
    </label>
  );
}

function SourceStep({
  draft, setDraft, pasted, onPaste, detected, checking, validation, onCheck, onNext,
}: {
  draft: DraftProvider;
  setDraft: React.Dispatch<React.SetStateAction<DraftProvider>>;
  pasted: string;
  onPaste: (text: string) => void;
  /** What detection could tell from the address: nothing, a full credential pair, or
      a panel root whose credentials the user still has to type. */
  detected: 'none' | 'credentials' | 'panel';
  checking: boolean;
  validation: ValidationResult | null;
  onCheck: () => void;
  onNext: () => void;
}) {
  const firstRef = useRef<HTMLTextAreaElement>(null);
  useEffect(() => firstRef.current?.focus(), []);

  return (
    <div style={card}>
      <Field label="Paste your playlist URL or Xtream address">
        <textarea className="aurora-field"
          ref={firstRef}
          value={pasted}
          onChange={(e) => onPaste(e.target.value)}
          placeholder="http://your-provider.example.com/get.php?username=…&password=…"
          rows={3}
          aria-label="Provider address"
          style={{ ...input, resize: 'vertical', fontFamily: 'ui-monospace, monospace', fontSize: 'var(--fs-sm)' }}
        />
      </Field>

      {detected !== 'none' && (
        <div
          style={{
            display: 'flex', alignItems: 'center', gap: 8, marginBottom: 'var(--sp-3)',
            fontSize: 'var(--fs-sm)',
            color: detected === 'credentials' ? 'var(--success)' : 'var(--text-muted)',
          }}
        >
          <Icon name={detected === 'credentials' ? 'check' : 'info'} size={15} />
          {detected === 'credentials'
            ? 'Recognised an Xtream address — username and password filled in for you.'
            : 'Looks like a panel login. Enter the username and password your provider gave you.'}
        </div>
      )}

      {/*
        Always offered, never inferred away. Detection can only read credentials out of
        a URL that carries them; a provider that gives you a bare host and a separate
        username and password — which is most of them — leaves nothing to detect. This
        used to mean those fields never appeared and the subscription could not be
        entered at all.
      */}
      {/* A group of buttons, so deliberately not a <label>: that element labels a form
          control, and a button inside one has its click forwarded to the control. */}
      <div role="group" aria-label="Provider type" style={{ marginBottom: 'var(--sp-3)' }}>
        <span
          style={{
            display: 'block', marginBottom: 5, fontSize: 'var(--fs-sm)',
            fontWeight: 600, color: 'var(--text-muted)',
          }}
        >
          Provider type
        </span>
        <div style={{ display: 'flex', gap: 6 }}>
          {([
            ['xtream', 'Xtream / panel login'],
            ['m3u', 'M3U playlist URL'],
          ] as const).map(([value, label]) => (
            <Button
              key={value}
              size="sm"
              variant={draft.kind === value ? 'primary' : 'secondary'}
              aria-pressed={draft.kind === value}
              onClick={() => setDraft((d) => ({ ...d, kind: value }))}
            >
              {label}
            </Button>
          ))}
        </div>
      </div>

      <Field label="Name this provider">
        <input className="aurora-field"
          value={draft.name}
          onChange={(e) => setDraft((d) => ({ ...d, name: e.target.value }))}
          placeholder="My subscription"
          aria-label="Provider name"
          style={input}
        />
      </Field>

      {draft.kind === 'xtream' && (
        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 'var(--sp-3)' }}>
          <Field label="Username">
            <input className="aurora-field"
              value={draft.username ?? ''}
              onChange={(e) => setDraft((d) => ({ ...d, username: e.target.value }))}
              aria-label="Username"
              style={input}
            />
          </Field>
          <Field label="Password">
            <input className="aurora-field"
              type="password"
              value={draft.password ?? ''}
              onChange={(e) => setDraft((d) => ({ ...d, password: e.target.value }))}
              aria-label="Password"
              style={input}
            />
          </Field>
        </div>
      )}

      {validation && (
        <div
          role="status"
          style={{
            display: 'flex', alignItems: 'flex-start', gap: 10, padding: 'var(--sp-3)',
            borderRadius: 'var(--r-md)', marginBottom: 'var(--sp-3)',
            background: validation.ok
              ? 'color-mix(in srgb, var(--success) 12%, transparent)'
              : 'color-mix(in srgb, var(--danger) 12%, transparent)',
            border: `1px solid ${validation.ok ? 'var(--success)' : 'var(--danger)'}`,
          }}
        >
          <Icon
            name={validation.ok ? 'check' : 'info'} size={17}
            style={{ color: validation.ok ? 'var(--success)' : 'var(--danger)', marginTop: 2 }}
          />
          <div style={{ fontSize: 'var(--fs-sm)' }}>
            <strong>{validation.message}</strong>
            {validation.detail && (
              <div style={{ color: 'var(--text-muted)', marginTop: 3 }}>{validation.detail}</div>
            )}
            {validation.ok && validation.daysUntilExpiry != null && (
              <div style={{ display: 'flex', gap: 6, marginTop: 7, flexWrap: 'wrap' }}>
                <Badge tone={validation.daysUntilExpiry < 7 ? 'live' : 'neutral'}>
                  {validation.daysUntilExpiry} days left
                </Badge>
                {validation.maxConnections != null && (
                  <Badge tone="outline">
                    {validation.activeConnections ?? 0}/{validation.maxConnections} connections
                  </Badge>
                )}
                {validation.isTrial && <Badge tone="accent">Trial</Badge>}
              </div>
            )}
          </div>
        </div>
      )}

      <div style={{ display: 'flex', gap: 'var(--sp-2)', justifyContent: 'flex-end' }}>
        <Button onClick={onCheck} disabled={checking || !draft.url}>
          {checking ? 'Checking…' : 'Check connection'}
        </Button>
        <Button variant="primary" onClick={onNext} disabled={!validation?.ok}>
          Continue
        </Button>
      </div>
    </div>
  );
}

function ContentStep({
  content, setContent, onBack, onImport,
}: {
  content: { live: boolean; vod: boolean; series: boolean };
  setContent: React.Dispatch<React.SetStateAction<{ live: boolean; vod: boolean; series: boolean }>>;
  onBack: () => void;
  onImport: () => void;
}) {
  const rows: [keyof typeof content, string, string][] = [
    ['live', 'Live TV', 'Channels and the programme guide'],
    ['vod', 'Movies', 'On-demand films'],
    ['series', 'Series', 'Shows, seasons and episodes'],
  ];
  const none = !content.live && !content.vod && !content.series;

  return (
    <div style={card}>
      <h2 style={{ margin: '0 0 var(--sp-4)', fontSize: 'var(--fs-lg)' }}>
        What should Aurora import?
      </h2>
      {rows.map(([key, title, hint]) => (
        <button
          key={key}
          role="switch"
          aria-checked={content[key]}
          aria-label={title}
          onClick={() => setContent((c) => ({ ...c, [key]: !c[key] }))}
          style={{
            display: 'flex', alignItems: 'center', gap: 'var(--sp-3)', width: '100%',
            padding: 'var(--sp-3)', marginBottom: 'var(--sp-2)', textAlign: 'left',
            borderRadius: 'var(--r-md)', cursor: 'pointer', color: 'inherit',
            border: `1px solid ${content[key] ? 'var(--accent)' : 'var(--border)'}`,
            background: content[key]
              ? 'color-mix(in srgb, var(--accent) 10%, transparent)'
              : 'transparent',
          }}
        >
          <div
            style={{
              width: 22, height: 22, borderRadius: 6, display: 'grid', placeItems: 'center',
              background: content[key] ? 'var(--accent)' : 'var(--surface)',
              color: 'var(--accent-text)', flexShrink: 0,
            }}
          >
            {content[key] && <Icon name="check" size={14} strokeWidth={3} />}
          </div>
          <div>
            <div style={{ fontWeight: 650 }}>{title}</div>
            <div style={{ fontSize: 'var(--fs-sm)', color: 'var(--text-muted)' }}>{hint}</div>
          </div>
        </button>
      ))}

      <div
        style={{
          display: 'flex', gap: 'var(--sp-2)', justifyContent: 'space-between',
          marginTop: 'var(--sp-4)',
        }}
      >
        <Button onClick={onBack} icon="chevronLeft">Back</Button>
        <Button variant="primary" onClick={onImport} disabled={none}>
          Import library
        </Button>
      </div>
    </div>
  );
}

function ImportingStep({ progress }: { progress: IngestProgress | null }) {
  const phase = progress?.phase ?? 'fetchingPlaylist';
  const pct =
    progress && progress.total > 0
      ? Math.round((progress.done / progress.total) * 100)
      : null;

  return (
    <div style={{ ...card, textAlign: 'center' }}>
      <div
        style={{
          width: 44, height: 44, margin: '0 auto var(--sp-4)', borderRadius: '50%',
          border: '3px solid var(--border)', borderTopColor: 'var(--accent)',
          animation: 'aurora-spin 0.9s linear infinite',
        }}
      />
      <style>{'@keyframes aurora-spin { to { transform: rotate(360deg); } }'}</style>

      <div role="status" style={{ fontSize: 'var(--fs-lg)', fontWeight: 650 }}>
        {PHASE_LABEL[phase]}
      </div>
      {pct != null && (
        <div style={{ marginTop: 'var(--sp-3)' }}>
          <div
            style={{
              height: 5, background: 'var(--border)', borderRadius: 'var(--r-full)',
              overflow: 'hidden',
            }}
          >
            <div
              style={{
                width: `${pct}%`, height: '100%', background: 'var(--accent)',
                transition: 'width var(--t-base) var(--ease)',
              }}
            />
          </div>
          <div style={{ marginTop: 6, fontSize: 'var(--fs-xs)', color: 'var(--text-faint)' }}>
            {progress?.done.toLocaleString()} of {progress?.total.toLocaleString()}
          </div>
        </div>
      )}
      <p style={{ marginTop: 'var(--sp-4)', color: 'var(--text-faint)', fontSize: 'var(--fs-sm)' }}>
        Large subscriptions can take a minute. You can keep using Aurora while this runs.
      </p>
    </div>
  );
}

function DoneStep({
  report, onFinished,
}: { report: SyncReport | null; onFinished: () => void }) {
  if (!report) {
    return <EmptyState title="Import finished" action={<Button onClick={onFinished}>Start watching</Button>} />;
  }
  const stats: [string, number][] = [
    ['Channels', report.channels],
    ['Movies', report.movies],
    ['Series', report.series],
    ['Episodes', report.episodes],
  ];
  const coverage =
    report.epgMatched + report.epgUnmatched.length > 0
      ? Math.round((report.epgMatched / (report.epgMatched + report.epgUnmatched.length)) * 100)
      : 0;

  return (
    <div style={card}>
      <div style={{ textAlign: 'center', marginBottom: 'var(--sp-4)' }}>
        <div style={{ color: 'var(--success)', marginBottom: 'var(--sp-2)' }}>
          <Icon name="check" size={34} strokeWidth={2.4} style={{ margin: '0 auto' }} />
        </div>
        <h2 style={{ margin: 0, fontSize: 'var(--fs-lg)' }}>Your library is ready</h2>
      </div>

      <div
        style={{
          display: 'grid', gridTemplateColumns: 'repeat(4, 1fr)', gap: 'var(--sp-2)',
          marginBottom: 'var(--sp-4)',
        }}
      >
        {stats.map(([label, value]) => (
          <div
            key={label}
            style={{
              padding: 'var(--sp-3)', borderRadius: 'var(--r-md)',
              background: 'var(--surface)', textAlign: 'center',
            }}
          >
            <div style={{ fontSize: 'var(--fs-xl)', fontWeight: 800 }}>
              {value.toLocaleString()}
            </div>
            <div style={{ fontSize: 'var(--fs-xs)', color: 'var(--text-faint)' }}>{label}</div>
          </div>
        ))}
      </div>

      <div style={{ fontSize: 'var(--fs-sm)', color: 'var(--text-muted)', marginBottom: 'var(--sp-4)' }}>
        Guide data matched {report.epgMatched.toLocaleString()} channels ({coverage}%).
        {report.epgUnmatched.length > 0 && ' The rest can be mapped by hand in Settings.'}
        {report.channelsMissing > 0 &&
          ` ${report.channelsMissing} channels your provider no longer lists were kept and flagged.`}
      </div>

      {report.warnings.length > 0 && (
        <details style={{ marginBottom: 'var(--sp-4)', fontSize: 'var(--fs-sm)' }}>
          <summary style={{ cursor: 'pointer', color: 'var(--warning)' }}>
            {report.warnings.length} warning{report.warnings.length === 1 ? '' : 's'}
          </summary>
          <ul style={{ margin: '8px 0 0', paddingLeft: 18, color: 'var(--text-muted)' }}>
            {report.warnings.slice(0, 8).map((w) => <li key={w}>{w}</li>)}
          </ul>
        </details>
      )}

      <Button variant="primary" onClick={onFinished} style={{ width: '100%' }} icon="play" iconFilled>
        Start watching
      </Button>
    </div>
  );
}
