import { create } from 'zustand';
import type { CatalogItem, PlayerState } from '@shared/ipc';
import { invoke, onPlayerState } from '@/ipc';

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

  setTheme: (t: Theme) => void;
  setDensity: (d: Density) => void;
  toggleAnimations: () => void;
  toggleHoverPreviews: () => void;
  openDetail: (i: CatalogItem | null) => void;
  setPalette: (open: boolean, query?: string) => void;
  setPlayer: (p: PlayerState) => void;
  showBanner: (channelId: number) => void;
  hideBanner: () => void;
  pushDigit: (d: string) => void;
  clearDigits: () => void;
}

function applyDom(theme: Theme, density: Density, animations: boolean) {
  const root = document.documentElement;
  root.dataset.theme = theme;
  root.dataset.density = density;
  root.dataset.motion = animations ? 'on' : 'off';
}

export const useUi = create<UiState>((set, get) => ({
  theme: 'dark',
  density: 'desktop',
  animations: !window.matchMedia?.('(prefers-reduced-motion: reduce)').matches,
  hoverPreviews: true,
  use24h: true,

  detail: null,
  paletteOpen: false,
  paletteQuery: '',
  player: null,
  banner: null,
  digits: '',

  setTheme: (theme) => { applyDom(theme, get().density, get().animations); set({ theme }); },
  setDensity: (density) => { applyDom(get().theme, density, get().animations); set({ density }); },
  toggleAnimations: () => {
    const animations = !get().animations;
    applyDom(get().theme, get().density, animations);
    set({ animations });
  },
  toggleHoverPreviews: () => set((s) => ({ hoverPreviews: !s.hoverPreviews })),
  openDetail: (detail) => set({ detail }),
  setPalette: (paletteOpen, paletteQuery = '') => set({ paletteOpen, paletteQuery }),
  setPlayer: (player) => set({ player }),
  showBanner: (channelId) =>
    set({ banner: { channelId, until: Date.now() + 5000 } }),
  hideBanner: () => set({ banner: null }),
  pushDigit: (d) => set((s) => ({ digits: (s.digits + d).slice(0, 4) })),
  clearDigits: () => set({ digits: '' }),
}));

/** Mirror host player state into the store (README §3: UI mirrors, never owns). */
export function bindPlayerState() {
  void invoke('player.state').then((s) => useUi.getState().setPlayer(s));
  return onPlayerState((s) => useUi.getState().setPlayer(s));
}
