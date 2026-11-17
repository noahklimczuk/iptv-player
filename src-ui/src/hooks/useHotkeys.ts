/**
 * The global key map (README §14.1, docs/PLAYER.md C31–C34). The one place keys are
 * read as commands.
 *
 * Registered once for the life of the app (C34): the handlers are read through a ref
 * when a key arrives, so a parent re-rendering, which while something plays is several
 * times a second, never removes and re-adds the listener.
 */
import { useEffect, useRef } from 'react';

export interface HotkeyHandlers {
  onDigit: (d: string) => void;
  onChannelUp: () => void;
  onChannelDown: () => void;
  onLastChannel: () => void;
  onGuide: () => void;
  onPalette: () => void;
  onBack: () => void;
  onPlayPause: () => void;
  /** `T`: pause live TV into the buffer, or rejoin the live edge. */
  onTimeshift: () => void;
  onSeek: (secs: number) => void;
  onVolume: (delta: number) => void;
  onMute: () => void;
  onFullscreen: () => void;
  /** `P`: the small picture, in a corner. */
  onPip: () => void;
  onInfo: () => void;
  onNavigate: (to: string) => void;
  /** Enter: commit a channel number being typed (C20). */
  onEnter?: () => void;
}

/**
 * Whether a keystroke belongs to a text field, where single-key shortcuts must not fire
 * (C31).
 *
 * The player's own sliders are not text fields (C32): after touching the seek or volume
 * bar, Space still pauses, M still mutes and the arrows still seek. Any other range, such
 * as a slider in Settings, keeps its keys.
 */
export function isTyping(t: EventTarget | null): boolean {
  const el = t as HTMLElement | null;
  if (!el?.tagName) return false;
  if (el.tagName === 'INPUT') {
    const type = (el as HTMLInputElement).type;
    if (type === 'range') return !el.closest('[data-player-overlay]');
    return !['checkbox', 'radio', 'button', 'submit', 'reset', 'color', 'file'].includes(type);
  }
  return el.tagName === 'TEXTAREA' || el.tagName === 'SELECT' || el.isContentEditable;
}

/** Space or Enter on a focused control presses that control, and nothing else (C6). */
function pressesFocused(e: KeyboardEvent): boolean {
  if (e.key !== ' ' && e.key !== 'Enter') return false;
  const el = e.target as HTMLElement | null;
  return !!el?.closest?.(
    'button, [role="button"], [role="switch"], [role="tab"], [role="option"], a[href], input[type="checkbox"], input[type="radio"]',
  );
}

const SECTIONS = ['/', '/live', '/guide', '/movies', '/series'] as const;

export function useHotkeys(handlers: HotkeyHandlers, enabled = true) {
  const h = useRef(handlers);
  h.current = handlers;
  const on = useRef(enabled);
  on.current = enabled;

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!on.current || e.defaultPrevented) return;
      const k = h.current;
      // Ctrl/Cmd+K works everywhere, typing included.
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'k') {
        e.preventDefault();
        k.onPalette();
        return;
      }
      if (isTyping(e.target) || pressesFocused(e)) return;
      // On a player slider the arrows are the player's, not the slider's (C32).
      if ((e.target as HTMLInputElement | null)?.type === 'range' && e.key.startsWith('Arrow')) {
        e.preventDefault();
      }
      if (e.altKey && /^[1-5]$/.test(e.key)) {
        e.preventDefault();
        k.onNavigate(SECTIONS[Number(e.key) - 1]!);
        return;
      }
      if (e.ctrlKey || e.metaKey || e.altKey) return;
      if (/^[0-9]$/.test(e.key)) {
        e.preventDefault();
        k.onDigit(e.key);
        return;
      }

      // Letters whatever the case, so Caps Lock does not turn the shortcuts off.
      const key = e.key.length === 1 ? e.key.toLowerCase() : e.key;
      switch (key) {
        case ' ':
        case 'k': e.preventDefault(); k.onPlayPause(); break;
        case 'Escape': k.onBack(); break;
        case 'Enter': k.onEnter?.(); break;
        case 'Backspace': e.preventDefault(); k.onLastChannel(); break;
        case 'PageUp': e.preventDefault(); k.onChannelUp(); break;
        case 'PageDown': e.preventDefault(); k.onChannelDown(); break;
        case 'ArrowLeft': k.onSeek(e.shiftKey ? -30 : -10); break;
        case 'ArrowRight': k.onSeek(e.shiftKey ? 30 : 10); break;
        case 'ArrowUp': k.onVolume(5); break;
        case 'ArrowDown': k.onVolume(-5); break;
        case 'j': k.onSeek(e.shiftKey ? -30 : -10); break;
        case 'l': k.onSeek(e.shiftKey ? 30 : 10); break;
        case 'm': k.onMute(); break;
        case 't': k.onTimeshift(); break;
        case 'f':
        case 'F11': e.preventDefault(); k.onFullscreen(); break;
        case 'p': k.onPip(); break;
        case 'i': k.onInfo(); break;
        case 'g': k.onGuide(); break;
        case '/': e.preventDefault(); k.onPalette(); break;
        default: break;
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);
}
