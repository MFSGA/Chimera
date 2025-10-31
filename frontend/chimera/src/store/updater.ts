import { type Update } from '@tauri-apps/plugin-updater';
import { atom } from 'jotai';
import { atomWithStorage } from 'jotai/utils';

export const UpdaterIgnoredAtom = atomWithStorage(
  'updaterIgnored',
  null as string | null,
);

export const UpdaterInstanceAtom = atom<Update | null>(null);
