/**
 * Global keyboard map (README §14.1). Every binding here is remappable in Settings in
 * the shipped product; this hook is the single place they are interpreted.
 */
import { useEffect } from 'react';

export interface HotkeyHandlers {
  onDigit: (d: string) => void;
  onChannelUp: () => void;
  onChannelDown: () => void;
  onLastChannel: () => void;
  onGuide: () => void;
  onPalette: () => void;
  onBack: () => void;
  onPlayPause: () => void;
  onSeek: (secs: number) => void;
  onVolume: (delta: number) => void;
  onMute: () => void;
  onFullscreen: () => void;
  onInfo: () => void;
  onNavigate: (to: string) => void;
}

/** True when focus is in a text field, where single-key shortcuts must not fire. */
function isTyping(t: EventTarget | null): boolean {
  const el = t as HTMLElement | null;
  if (!el) return false;
  const tag = el.tagName;
  return tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT' || el.isContentEditable;
}

export function useHotkeys(h: HotkeyHandlers, enabled = true) {
  useEffect(() => {
    if (!enabled) return;

    const onKey = (e: KeyboardEvent) => {
      // Ctrl/Cmd combos work everywhere, including while typing.
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'k') {
        e.preventDefault();
        h.onPalette();
        return;
      }
      if (isTyping(e.target)) return;
      if (e.altKey && /^[1-5]$/.test(e.key)) {
        e.preventDefault();
        h.onNavigate(['/', '/live', '/guide', '/movies', '/series'][Number(e.key) - 1]!);
        return;
      }
      if (e.ctrlKey || e.metaKey || e.altKey) return;

      if (/^[0-9]$/.test(e.key)) { e.preventDefault(); h.onDigit(e.key); return; }

      switch (e.key) {
        case ' ':
        case 'k': e.preventDefault(); h.onPlayPause(); break;
        case 'Escape': h.onBack(); break;
        case 'Backspace': e.preventDefault(); h.onLastChannel(); break;
        case 'PageUp': e.preventDefault(); h.onChannelUp(); break;
        case 'PageDown': e.preventDefault(); h.onChannelDown(); break;
        case 'ArrowLeft': h.onSeek(e.shiftKey ? -30 : -10); break;
        case 'ArrowRight': h.onSeek(e.shiftKey ? 30 : 10); break;
        case 'ArrowUp': h.onVolume(5); break;
        case 'ArrowDown': h.onVolume(-5); break;
        case 'j': h.onSeek(-10); break;
        case 'l': h.onSeek(10); break;
        case 'm': h.onMute(); break;
        case 'f': h.onFullscreen(); break;
        case 'i': h.onInfo(); break;
        case 'g': h.onGuide(); break;
        case '/': e.preventDefault(); h.onPalette(); break;
        default: break;
      }
    };

    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [h, enabled]);
}
