import { useCallback, useState } from 'react';

export function useLocalStorage<T>(key: string, initialValue: T) {
  const [value, setValue] = useState<T>(() => {
    const stored = localStorage.getItem(key);
    if (stored === null) return initialValue;

    try {
      return JSON.parse(stored) as T;
    } catch {
      return initialValue;
    }
  });

  const setStoredValue = useCallback(
    (nextValue: T | ((previous: T) => T)) => {
      setValue((previous) => {
        const resolved =
          typeof nextValue === 'function'
            ? (nextValue as (previous: T) => T)(previous)
            : nextValue;
        localStorage.setItem(key, JSON.stringify(resolved));
        return resolved;
      });
    },
    [key],
  );

  return [value, setStoredValue] as const;
}
