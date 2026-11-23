import {
  useClashSettings,
  type DnsFeatures,
  type DnsSettings,
  type FallbackFilter,
  type OneOrMany,
} from '@chimera/interface';
import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type PropsWithChildren,
} from 'react';

/**
 * Serde supplies every field when reading persisted DNS settings, although
 * Specta marks them optional because Rust accepts incomplete input.
 */
export type DnsValues = Required<Omit<DnsSettings, 'fallback-filter'>> & {
  'fallback-filter': Required<FallbackFilter>;
};
export type Hosts = Record<string, OneOrMany>;
export type DnsDraft = { dns: DnsValues; hosts: Hosts };

type DnsDraftContextValue = {
  draft: DnsDraft;
  features: DnsFeatures;
  dirty: boolean;
  saving: boolean;
  updateDns: (patch: Partial<DnsValues>) => void;
  setHosts: (hosts: Hosts) => void;
  replaceDraft: (draft: DnsDraft) => void;
  discard: () => void;
  save: () => Promise<void>;
};

const DnsDraftContext = createContext<DnsDraftContextValue | null>(null);

export function useDnsDraft() {
  const value = useContext(DnsDraftContext);
  if (!value) throw new Error('DNS draft must be inside DnsDraftProvider');
  return value;
}

export function DnsDraftProvider({
  saved,
  features,
  children,
}: PropsWithChildren<{ saved: DnsDraft; features: DnsFeatures }>) {
  const { upsert } = useClashSettings();
  const [draft, setDraft] = useState(saved);

  // The source of truth is the persisted settings. A confirmed save or change
  // from another window refreshes the draft, just as in ref's DNS editor.
  useEffect(() => setDraft(saved), [saved]);
  const dirty = useMemo(
    () => JSON.stringify(draft) !== JSON.stringify(saved),
    [draft, saved],
  );

  const updateDns = useCallback(
    (patch: Partial<DnsValues>) =>
      setDraft((current) => ({
        ...current,
        dns: { ...current.dns, ...patch },
      })),
    [],
  );
  const setHosts = useCallback(
    (hosts: Hosts) => setDraft((current) => ({ ...current, hosts })),
    [],
  );
  const discard = useCallback(() => setDraft(saved), [saved]);
  const save = useCallback(async () => {
    // Both tables are committed by one typed ClashConfigPatch. Do not patch
    // just one list and accidentally leave the other half unsaved.
    await upsert.mutateAsync({ dns: draft.dns, hosts: draft.hosts });
  }, [upsert, draft]);

  const value = useMemo(
    () => ({
      draft,
      features,
      dirty,
      saving: upsert.isPending,
      updateDns,
      setHosts,
      replaceDraft: setDraft,
      discard,
      save,
    }),
    [
      draft,
      features,
      dirty,
      upsert.isPending,
      updateDns,
      setHosts,
      discard,
      save,
    ],
  );

  return (
    <DnsDraftContext.Provider value={value}>
      {children}
    </DnsDraftContext.Provider>
  );
}

export function useSavedDns() {
  const { query } = useClashSettings();
  const saved = useMemo(
    () =>
      query.data?.dns && query.data.hosts
        ? { dns: query.data.dns as DnsValues, hosts: query.data.hosts }
        : undefined,
    [query.data],
  );
  return { saved, query };
}
