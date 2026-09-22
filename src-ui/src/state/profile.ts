/**
 * The active profile (README §11).
 *
 * Everything per-profile — progress, My List, favorites, skip preferences — is keyed
 * by this. It used to be hardcoded to 1 in a dozen call sites; it now lives here so
 * switching profile actually changes what the app shows.
 */
import { create } from 'zustand';
import type { ParentalSettings, Profile } from '@shared/ipc';
import { invoke } from '@/ipc';

interface ProfileState {
  profiles: Profile[];
  active: Profile | null;
  parental: ParentalSettings | null;
  /** True until a profile has been chosen, so the picker shows on launch. */
  picking: boolean;
  load: () => Promise<void>;
  activate: (profile: Profile) => void;
  setPicking: (picking: boolean) => void;
}

export const useProfile = create<ProfileState>((set, get) => ({
  profiles: [],
  active: null,
  parental: null,
  picking: true,

  load: async () => {
    const [profiles, parental] = await Promise.all([
      invoke('profiles.list'),
      invoke('profiles.parental'),
    ]);
    // With exactly one unlocked profile there is nobody to choose between, so skip
    // the picker rather than making every launch a two-click affair.
    const onlyOne = profiles.length === 1 && !profiles[0]!.hasPin;
    set({
      profiles,
      parental,
      active: onlyOne ? profiles[0]! : get().active,
      picking: onlyOne ? false : get().picking,
    });
  },

  activate: (active) => set({ active, picking: false }),
  setPicking: (picking) => set({ picking }),
}));

/** The id to key per-profile data by. Falls back to 1, which always exists. */
export const activeProfileId = () => useProfile.getState().active?.id ?? 1;
