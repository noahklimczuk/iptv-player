import { create } from 'zustand';
import type { CatalogItem, PlayerState } from '@shared/ipc';
import { invoke, onPlayerState } from '@/ipc';
import { report } from '@/lib/errors';

export type Theme = 'dark' | 'oled' | 'light' | 'contrast';
export type Density = 'desktop' | 'tv';

interface UiState {
  theme: Theme;
  density: Density;
  animations: boolean;
  hoverPreviews: boolean;
  use24h: boolean;

  /** Currently open detail modal, if any (README §8.4). */
  detail: CatalogItem | null;
  paletteOpen: boolean;
  /** Seed text for the palette, so "Search this title" lands on results. */
  paletteQuery: string;

  player: PlayerState | null;
  /** Channel banner shown briefly after a zap (README §7.2). */
  banner: { channelId: number; until: number } | null;
  /** Digits typed for direct channel entry (README §7.2). */
  digits: string;
  /**
   * Bumped whenever something has happened that the library screens should re-read.
   *
   * Home fetches its rails once and keeps them, which is right — they are expensive
   * and the library does not usually move. But watching something *does* move it:
   * Continue Watching gains a card and the recommendations change, and neither
   * appeared until the app was restarted, because closing the player navigates
   * nowhere and so remounts nothing. A version somewhere central is the cheapest way
   * for the screens that care to notice.
   */
  catalogVersion: number;

  setTheme: (t: Theme) => void;
  setDensity: (d: Density) => void;
  toggleAnimations: () => void;
  toggleHoverPreviews: () => void;
  /** 24-hour or AM/PM, which `clockTime` reads and nothing used to set (R-136). */
  toggle24h: () => void;
  openDetail: (i: CatalogItem | null) => void;
  setPalette: (open: boolean, query?: string) => void;
  setPlayer: (p: PlayerState) => void;
  showBanner: (channelId: number) => void;
  hideBanner: () => void;
  pushDigit: (d: string) => void;
  /** Say that the library has moved. */
  bumpCatalog: () => void;
  clearDigits: () => void;
}

function applyDom(theme: Theme, density: Density, animations: boolean) {
  const root = document.documentElement;
  root.dataset.theme = theme;
  root.dataset.density = density;
  root.dataset.motion = animations ? 'on' : 'off';
}

/**
 * Where the four Appearance choices are kept, so they survive the window closing.
 *
 * They were not kept anywhere. `useUi` was a plain `create` with no persistence and a
 * hardcoded initial state, and every handler in the Appearance panel wrote only to the
 * in-memory store -- so picking the OLED theme, or TV density, lasted until the app was
 * closed. **Density is the one that mattered**: its own hint says "TV mode enlarges
 * everything for a 10-foot UI", which is what the app is for, so anybody running Aurora
 * on a television had to set it again on every launch. Everything else in Settings
 * persists through the host, which made this the one panel that did not (R-87).
 *
 * `localStorage` rather than a host command: these are per-machine display preferences,
 * not library data, and the machine is the right scope for them -- the same library
 * opened on a television and on a laptop wants two different densities. It also means
 * the choice is applied on the first paint, before any IPC could have answered.
 *
 * Every access is wrapped: the accessor throws outright in a WebView with site data
 * blocked, and a thumbnail capture or a private window can hand back an empty store. A
 * forgotten setting is a small cost; a boot that dies reading one is not.
 */
const SAVED_KEY = 'aurora.appearance';

interface SavedAppearance {
  theme: Theme;
  density: Density;
  animations: boolean;
  hoverPreviews: boolean;
  use24h: boolean;
}

const THEMES: Theme[] = ['dark', 'oled', 'light', 'contrast'];
const DENSITIES: Density[] = ['desktop', 'tv'];

function loadAppearance(): Partial<SavedAppearance> {
  let raw: string | null;
  try {
    raw = window.localStorage.getItem(SAVED_KEY);
  } catch {
    return {};
  }
  if (!raw) return {};
  try {
    const parsed: unknown = JSON.parse(raw);
    if (typeof parsed !== 'object' || parsed === null) return {};
    const v = parsed as Record<string, unknown>;
    const out: Partial<SavedAppearance> = {};
    // Checked against the allowed values rather than trusted. Whatever is in this key
    // is a string somebody could have edited, and an unknown theme would go straight
    // into `data-theme`, where it matches no stylesheet and leaves the app unstyled.
    if (THEMES.includes(v.theme as Theme)) out.theme = v.theme as Theme;
    if (DENSITIES.includes(v.density as Density)) out.density = v.density as Density;
    if (typeof v.animations === 'boolean') out.animations = v.animations;
    if (typeof v.hoverPreviews === 'boolean') out.hoverPreviews = v.hoverPreviews;
    if (typeof v.use24h === 'boolean') out.use24h = v.use24h;
    return out;
  } catch {
    return {};
  }
}

function saveAppearance(a: SavedAppearance) {
  try {
    window.localStorage.setItem(SAVED_KEY, JSON.stringify(a));
  } catch {
    // Nothing to do and nothing worth telling the viewer: the setting still applies to
    // this session, it just will not be there next time.
  }
}

const saved = loadAppearance();

/** The initial appearance: what was chosen last time, or what the machine implies. */
const initialAppearance: SavedAppearance = {
  theme: saved.theme ?? 'dark',
  density: saved.density ?? 'desktop',
  animations:
    saved.animations ?? !window.matchMedia?.('(prefers-reduced-motion: reduce)').matches,
  hoverPreviews: saved.hoverPreviews ?? true,
  use24h: saved.use24h ?? true,
};

export const useUi = create<UiState>((set, get) => ({
  theme: initialAppearance.theme,
  density: initialAppearance.density,
  animations: initialAppearance.animations,
  hoverPreviews: initialAppearance.hoverPreviews,
  use24h: initialAppearance.use24h,

  detail: null,
  paletteOpen: false,
  paletteQuery: '',
  player: null,
  banner: null,
  digits: '',
  catalogVersion: 0,

  setTheme: (theme) => {
    applyDom(theme, get().density, get().animations);
    set({ theme });
    remember(get());
  },
  setDensity: (density) => {
    applyDom(get().theme, density, get().animations);
    set({ density });
    remember(get());
  },
  toggleAnimations: () => {
    const animations = !get().animations;
    applyDom(get().theme, get().density, animations);
    set({ animations });
    remember(get());
  },
  toggleHoverPreviews: () => {
    set((s) => ({ hoverPreviews: !s.hoverPreviews }));
    remember(get());
  },
  toggle24h: () => {
    set((s) => ({ use24h: !s.use24h }));
    remember(get());
  },
  openDetail: (detail) => set({ detail }),
  setPalette: (paletteOpen, paletteQuery = '') => set({ paletteOpen, paletteQuery }),
  setPlayer: (player) => set({ player }),
  showBanner: (channelId) =>
    set({ banner: { channelId, until: Date.now() + 5000 } }),
  hideBanner: () => set({ banner: null }),
  pushDigit: (d) => set((s) => ({ digits: (s.digits + d).slice(0, 4) })),
  bumpCatalog: () => set((s) => ({ catalogVersion: s.catalogVersion + 1 })),
  clearDigits: () => set({ digits: '' }),
}));

/** Keep the five Appearance choices, whatever the setter just changed. */
function remember(s: UiState) {
  saveAppearance({
    theme: s.theme,
    density: s.density,
    animations: s.animations,
    hoverPreviews: s.hoverPreviews,
    use24h: s.use24h,
  });
}

/**
 * Apply them to `<html>` now, rather than when a setting is next touched.
 *
 * `applyDom` only ran from a setter, so a fresh window carried no `data-density` and no
 * `data-motion` at all -- harmless while the defaults were hardcoded, since
 * `data-density='tv'` is the only density override and the reduced-motion rules are
 * duplicated inside an `@media` block. The moment a *stored* choice exists, the absence
 * is the bug: the store would say `tv` and the document would not.
 */
applyDom(
  initialAppearance.theme,
  initialAppearance.density,
  initialAppearance.animations,
);

/** Mirror host player state into the store (README §3: UI mirrors, never owns). */
export function bindPlayerState() {
  invoke('player.state')
    .then((s) => useUi.getState().setPlayer(s))
    .catch(report('Could not read the player state'));
  return onPlayerState((s) => useUi.getState().setPlayer(s));
}
