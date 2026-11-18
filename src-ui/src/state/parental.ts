/**
 * Parental locks and the gate's state, for the screens that show them (README §11).
 *
 * The host decides what may be seen and played (`aurora_db::repo::parental`); this only
 * mirrors enough of it to draw a padlock on a locked channel and to fill the parental
 * settings. Reloaded after every change and whenever the PIN dialog opens something.
 */
import { create } from 'zustand';
import type { LockKind, ParentalLock, ParentalState } from '@shared/ipc';
import { invoke } from '@/ipc';
import { useProfile } from '@/state/profile';

interface ParentalStore {
  locks: ParentalLock[];
  state: ParentalState | null;
  load: () => Promise<void>;
  setLock: (kind: LockKind, value: string, locked: boolean) => Promise<void>;
}

export const useParental = create<ParentalStore>((set) => ({
  locks: [],
  state: null,
  load: async () => {
    const [locks, state, parental] = await Promise.all([
      invoke('parental.locks'),
      invoke('parental.state'),
      invoke('profiles.parental'),
    ]);
    set({ locks, state });
    // The household settings live in the profile store, read once at launch; anything
    // that reloads the gate refreshes them too, so a padlock and a locked Settings page
    // never go by a PIN that has since been set or removed.
    useProfile.setState({ parental });
  },
  setLock: async (kind, value, locked) => {
    const locks = await invoke('parental.setLock', { kind, value, locked });
    set({ locks });
  },
}));

/** Whether a channel is locked, by itself or by the category it is filed under. */
export function isChannelLocked(
  locks: ParentalLock[],
  channel: { id: number; group: string | null },
): boolean {
  const id = String(channel.id);
  return locks.some((l) =>
    (l.kind === 'channel' && l.value === id)
    || (l.kind === 'category' && channel.group != null && l.value === channel.group));
}
