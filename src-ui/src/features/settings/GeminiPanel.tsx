/**
 * The Gemini key, and the one place that says why the rail is not there.
 *
 * The rail itself is silent by design — no key, nothing watched, the model refused, or
 * none of its suggestions are in this library are all reasons for it not to exist rather
 * than reasons to apologise on somebody's home screen. So this screen has to carry the
 * explanation, and it is the only place that does.
 */
import { useCallback, useEffect, useState } from 'react';
import { Button, FIELD } from '@/components/Primitives';
import { useCommand } from '@/hooks/useCommand';
import { invoke } from '@/ipc';
import { report } from '@/lib/errors';
import { useProfile } from '@/state/profile';

export function GeminiPanel() {
  const profileId = useProfile((s) => s.active?.id ?? 1);
  const { data: status, reload } = useCommand('gemini.status', { profileId }, [profileId]);
  const [key, setKey] = useState('');
  const [busy, setBusy] = useState(false);
  /** What the last attempt to generate said, good or bad. */
  const [result, setResult] = useState<{ ok: boolean; text: string } | null>(null);

  useEffect(() => setResult(null), [profileId]);

  const save = useCallback(async () => {
    setBusy(true);
    try {
      await invoke('gemini.setKey', { key });
      setKey('');
      setResult(null);
      reload();
    } catch (e) {
      report('Could not save the key')(e);
    } finally {
      setBusy(false);
    }
  }, [key, reload]);

  /** Ask for a fresh set, and say plainly what came back. */
  const test = useCallback(async () => {
    setBusy(true);
    setResult(null);
    try {
      const picks = await invoke('gemini.recommendations', { profileId, refresh: true });
      setResult({
        ok: picks.items.length > 0,
        text: picks.items.length > 0
          ? `Suggested ${picks.items.length + picks.notInLibrary} titles; `
            + `${picks.items.length} are in your library and are on the home screen now.`
          : `Suggested ${picks.notInLibrary} titles and your provider carries none of them. `
            + 'That is a subscription that does not stock what you watch, not a broken key.',
      });
    } catch (e) {
      setResult({ ok: false, text: e instanceof Error ? e.message : String(e) });
    } finally {
      setBusy(false);
    }
  }, [profileId]);

  return (
    <div style={{ display: 'grid', gap: 'var(--sp-3)' }}>
      <div style={{ color: 'var(--text-muted)', fontSize: 'var(--fs-sm)', lineHeight: 1.5 }}>
        Suggests films and shows from what you have watched, then keeps only the ones your
        provider actually carries. What leaves this computer is the titles you have
        watched, their years and genres, how much of each you watched and whether you
        liked it — nothing about your provider, your sign-in or the rest of your library.
      </div>

      {status && !status.hasKey && (
        <div style={{ color: 'var(--text-muted)', fontSize: 'var(--fs-sm)' }}>
          No key yet. One is free from{' '}
          <span style={{ color: 'var(--accent)' }}>aistudio.google.com/apikey</span>.
        </div>
      )}

      {status?.hasKey && status.keyIsBuiltIn && (
        <div style={{ color: 'var(--text-muted)', fontSize: 'var(--fs-sm)' }}>
          Using the key built into this build. Entering your own replaces it, and carries
          your own rate limit rather than sharing one.
        </div>
      )}

      {status?.hasKey && !status.keyIsPersistent && (
        <div style={{ color: 'var(--warning)', fontSize: 'var(--fs-sm)' }}>
          There is no OS credential store on this platform, so a key saved here lasts only
          until Aurora closes.
        </div>
      )}

      {status?.hasKey && !status.hasHistory && (
        <div style={{ color: 'var(--text-muted)', fontSize: 'var(--fs-sm)' }}>
          Nothing watched yet. These are built from your history, so the rail appears once
          there is one.
        </div>
      )}

      <div style={{ display: 'flex', gap: 'var(--sp-2)', alignItems: 'center' }}>
        <input
          className="aurora-field"
          type="password"
          value={key}
          onChange={(e) => setKey(e.target.value)}
          placeholder={status?.hasKey ? 'Replace the saved key…' : 'Paste your API key…'}
          aria-label="Gemini API key"
          autoComplete="off"
          spellCheck={false}
          style={{ ...FIELD, flex: 1 }}
        />
        {/* Not "Save", and not "Save key" either. The metadata panel on this same screen
            has a Save, so one word would be two buttons a screen reader cannot tell apart
            — and anything *containing* "Save" still collides for anyone matching by
            substring, which is how the existing journey for that panel finds its button. */}
        <Button
          size="sm"
          variant="primary"
          disabled={busy || !key.trim()}
          onClick={() => void save()}
        >
          Use this key
        </Button>
        {status?.hasKey && (
          <Button
            size="sm"
            disabled={busy}
            onClick={() => { setKey(''); void invoke('gemini.setKey', { key: '' }).then(reload); }}
          >
            Remove key
          </Button>
        )}
      </div>

      {status?.hasKey && (
        <div>
          <Button size="sm" disabled={busy} onClick={() => void test()}>
            {busy ? 'Asking…' : 'Get recommendations now'}
          </Button>
        </div>
      )}

      {result && (
        <div
          role="status"
          style={{
            padding: 'var(--sp-3)',
            borderRadius: 'var(--r-md)',
            fontSize: 'var(--fs-sm)',
            background: result.ok
              ? 'color-mix(in srgb, var(--success) 12%, transparent)'
              : 'color-mix(in srgb, var(--danger) 12%, transparent)',
            border: `1px solid ${result.ok ? 'var(--success)' : 'var(--danger)'}`,
          }}
        >
          {result.text}
        </div>
      )}
    </div>
  );
}
