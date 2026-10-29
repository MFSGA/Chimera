import {
  getStorageItem,
  removeStorageItem,
  setStorageItem,
} from '@chimera/interface';
import { createJSONStorage } from 'jotai/utils';
import { type AsyncStringStorage } from 'jotai/vanilla/utils/atomWithStorage';

const subscribers: Map<
  string,
  Set<(newValue: string | null) => void>
> = new Map();

const WEB_STORAGE_KEY_PREFIX = 'web:';

export function dispatchStorageValueChanged(
  key: string,
  newValue: string | null,
) {
  if (subscribers.has(key)) {
    const set = subscribers.get(key);
    set!.forEach((callback) => {
      callback(newValue);
    });
  }
}

export function dispatchLegacyStorageValueChanged(
  key: string,
  newValue: string | null,
) {
  const legacyKey = key.startsWith(WEB_STORAGE_KEY_PREFIX)
    ? key.slice(WEB_STORAGE_KEY_PREFIX.length)
    : key;
  dispatchStorageValueChanged(
    legacyKey,
    typeof newValue === 'string' ? JSON.parse(newValue) : newValue,
  );
}

export const ChimeraStorage = {
  getItem(key) {
    return getStorageItem(key);
  },
  setItem(key, newValue) {
    return setStorageItem(key, newValue);
  },
  removeItem(key) {
    return removeStorageItem(key);
  },
  subscribe(key, callback) {
    if (!subscribers.has(key)) {
      subscribers.set(key, new Set());
    }
    const set = subscribers.get(key);
    set!.add(callback);
    return () => {
      if (subscribers.has(key)) {
        const set = subscribers.get(key);
        set!.delete(callback);
        if (set!.size === 0) {
          subscribers.delete(key);
        }
      }
    };
  },
} satisfies AsyncStringStorage;

export const ChimeraJSONStorage = createJSONStorage(() => ChimeraStorage);
