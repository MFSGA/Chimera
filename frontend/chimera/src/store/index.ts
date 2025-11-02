import { atom } from 'jotai';
import { atomWithStorage } from 'jotai/utils';
import { FileRouteTypes } from '@/routeTree.gen';

const atomWithLocalStorage = <T>(key: string, initialValue: T) => {
  const getInitialValue = (): T => {
    const item = localStorage.getItem(key);

    return item ? JSON.parse(item) : initialValue;
  };

  const baseAtom = atom<T>(getInitialValue());

  const derivedAtom = atom(
    (get) => get(baseAtom),
    (get, set, update: T | ((prev: T) => T)) => {
      const nextValue =
        typeof update === 'function'
          ? (update as (prev: T) => T)(get(baseAtom))
          : update;

      set(baseAtom, nextValue);

      localStorage.setItem(key, JSON.stringify(nextValue));
    },
  );

  return derivedAtom;
};

export const memorizedRoutePathAtom = atomWithStorage<
  FileRouteTypes['fullPaths'] | null
>('memorizedRoutePathAtom', null);

export const proxyGroupAtom = atomWithLocalStorage<{
  selector: number | null;
}>('proxyGroupAtom', {
  selector: 0,
});
