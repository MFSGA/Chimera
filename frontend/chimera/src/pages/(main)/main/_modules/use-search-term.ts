import { useIsPresent } from 'motion/react';
import { useEffect, useRef, useState } from 'react';

/** Keep a page search term in its route entry after typing pauses. */
export function useSearchTerm(
  q: string | undefined,
  writeQuery: (q: string | undefined) => void,
) {
  const [search, setSearch] = useState(q ?? '');
  const known = useRef(q);
  const [debouncedSearch, setDebouncedSearch] = useState(search);
  const isPresent = useIsPresent();

  useEffect(() => {
    if (q !== known.current) {
      known.current = q;
      setSearch(q ?? '');
    }
  }, [q]);

  useEffect(() => {
    const timeout = window.setTimeout(() => setDebouncedSearch(search), 300);
    return () => window.clearTimeout(timeout);
  }, [search]);

  useEffect(() => {
    const next = debouncedSearch || undefined;

    if (!isPresent || debouncedSearch !== search || next === known.current) {
      return;
    }

    known.current = next;
    writeQuery(next);
  }, [debouncedSearch, search, isPresent, writeQuery]);

  return [search, setSearch] as const;
}
