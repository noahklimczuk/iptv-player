/**
 * The parental PIN, asked for wherever the host refuses for want of it (README §11).
 *
 * Mounted once. `invoke` calls it when a command comes back locked or needing the PIN
 * (`setParentalPinAsker`), and retries the command if the PIN opens it -- so a locked
 * channel asks for the PIN and then plays, and a guarded change asks and then saves,
 * without either call site knowing.
 */
import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke, setParentalPinAsker, type ParentalReason } from '@/ipc';
import { useFocusTrap } from '@/hooks/useFocusTrap';
import { useParental } from '@/state/parental';
import { PinPad } from '@/features/profiles/PinPad';

export function ParentalPinDialog() {
  const [asking, setAsking] = useState<{
    reason: ParentalReason;
    resolve: (opened: boolean) => void;
  } | null>(null);
  const ref = useRef<HTMLDivElement>(null);
  useFocusTrap(ref, asking != null);

  useEffect(() => setParentalPinAsker((reason) => new Promise<boolean>((resolve) => {
    setAsking({ reason, resolve });
  })), []);

  const close = useCallback((opened: boolean) => {
    setAsking((now) => {
      now?.resolve(opened);
      return null;
    });
    if (opened) useParental.getState().load().catch(() => {});
  }, []);

  useEffect(() => {
    if (!asking) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Escape') return;
      // Before anything behind the dialog hears it: Escape here means "never mind",
      // not "leave the player".
      e.stopPropagation();
      close(false);
    };
    window.addEventListener('keydown', onKey, true);
    return () => window.removeEventListener('keydown', onKey, true);
  }, [asking, close]);

  if (!asking) return null;

  return (
    <div
      style={{
        position: 'fixed', inset: 0, zIndex: 400, display: 'grid', placeItems: 'center',
        background: 'var(--scrim-dialog)', backdropFilter: 'blur(4px)',
      }}
    >
      <div
        ref={ref}
        tabIndex={-1}
        role="dialog"
        aria-modal="true"
        aria-label="Parental PIN"
        style={{
          background: 'var(--bg-elevated)', borderRadius: 40, boxShadow: 'var(--shadow-4)',
          minWidth: 360,
        }}
      >
        <PinPad
          layout="dialog"
          heading={asking.reason === 'watch'
            ? 'Enter the parental PIN to watch this'
            : 'Enter the parental PIN'}
          badge="🔒"
          badgeColor="var(--surface)"
          verify={(pin) => invoke('parental.unlock', { pin })}
          failure="Could not check the parental PIN"
          onCancel={() => close(false)}
          onUnlocked={() => close(true)}
        />
      </div>
    </div>
  );
}
