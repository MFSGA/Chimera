import { useProfile } from '@chimera/interface';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useBlockTask } from '@/components/providers/block-task-provider';
import type { TransformChainLayout } from './transform-chain-column';
import {
  categoryProfiles,
  isChainProfile,
  type TransformProfile,
} from './utils';

type ScriptOrMergeProfile = TransformProfile;

enum ColumnType {
  Active = 'active',
  Inactive = 'inactive',
}

const COLUMN_TYPES = [ColumnType.Active, ColumnType.Inactive] as const;

type ChainColumns = Record<ColumnType, string[]>;

type DragSession = {
  columns: ChainColumns;
  profiles: ScriptOrMergeProfile[];
};

const sameOrder = (left: string[], right: string[]) =>
  left.length === right.length &&
  left.every((uid, index) => uid === right[index]);

const sameColumns = (left: ChainColumns, right: ChainColumns) =>
  COLUMN_TYPES.every((type) => sameOrder(left[type], right[type]));

/** Server order wins; uids without a matching candidate are dropped. */
const deriveColumns = (
  transforms: string[],
  candidateUids: string[],
): ChainColumns => {
  const known = new Set(candidateUids);
  const active = transforms.filter((uid) => known.has(uid));
  const selected = new Set(active);

  return {
    [ColumnType.Active]: active,
    [ColumnType.Inactive]: candidateUids.filter((uid) => !selected.has(uid)),
  };
};

/** Keeps the draft order while candidates are added or removed. */
const reconcileCandidates = (
  draft: ChainColumns,
  candidateUids: string[],
): ChainColumns => {
  const known = new Set(candidateUids);
  const active = draft[ColumnType.Active].filter((uid) => known.has(uid));
  const selected = new Set(active);
  const inactive = draft[ColumnType.Inactive].filter(
    (uid) => known.has(uid) && !selected.has(uid),
  );
  const present = new Set([...active, ...inactive]);

  return {
    [ColumnType.Active]: active,
    [ColumnType.Inactive]: [
      ...inactive,
      ...candidateUids.filter((uid) => !present.has(uid)),
    ],
  };
};

/**
 * Draft of a transform chain. Ref's state ownership is shared between the
 * global and scoped editors. This compatibility phase additionally retains
 * Chimera's inactive column and its candidate ordering while those views
 * converge on ref's single enabled list.
 */
export function useTransformChainDraft({
  transforms: currentTransforms,
  taskKey,
  onApply,
  layout,
}: {
  transforms: string[];
  taskKey: string;
  onApply: (transforms: string[]) => Promise<unknown>;
  layout: TransformChainLayout;
}) {
  const {
    query: { data: profiles },
  } = useProfile();

  const dataReady = profiles != null;

  // Candidate chain items = every Transform profile (Overlay / Script).
  const candidateProfiles = useMemo<ScriptOrMergeProfile[]>(() => {
    const candidates = (profiles?.items ?? []).filter(isChainProfile);
    if (layout === 'stacked') return candidates;

    return Object.values(categoryProfiles(candidates))
      .flat()
      .filter(isChainProfile);
  }, [profiles?.items, layout]);

  const candidateUids = useMemo(
    () => candidateProfiles.map((item) => item.uid),
    [candidateProfiles],
  );

  const serverChains = useMemo(
    () => deriveColumns(currentTransforms, candidateUids),
    [currentTransforms, candidateUids],
  );

  const [draft, setDraft] = useState<ChainColumns>(serverChains);

  // The draft is checked alongside the config because a corrected server
  // snapshot lands one commit before the sync effect replaces the draft, and
  // two sortables sharing a uid clobber each other in the dnd-kit registry.
  const hasDuplicates =
    new Set(currentTransforms).size !== currentTransforms.length ||
    new Set(draft[ColumnType.Active]).size !== draft[ColumnType.Active].length;

  const draftRef = useRef(draft);
  // null until the first server snapshot is adopted, so the initial load wins.
  const adoptedActiveRef = useRef<string[] | null>(null);
  const dragSessionRef = useRef<DragSession | null>(null);
  const [dragSession, setDragSession] = useState<DragSession | null>(null);
  const savingRef = useRef(false);

  const publishDraft = useCallback((next: ChainColumns) => {
    draftRef.current = next;
    setDraft(next);
  }, []);

  useEffect(() => {
    if (!dataReady || dragSessionRef.current) return;

    const serverActive = serverChains[ColumnType.Active];
    const serverChanged =
      adoptedActiveRef.current === null ||
      !sameOrder(adoptedActiveRef.current, serverActive);

    // A changed chain is the server's call; a changed candidate list only adds
    // or removes entries and must not discard the draft order.
    const next = serverChanged
      ? serverChains
      : reconcileCandidates(draftRef.current, candidateUids);

    adoptedActiveRef.current = serverActive;

    if (!sameColumns(next, draftRef.current)) {
      publishDraft(next);
    }
  }, [serverChains, candidateUids, dragSession, dataReady, publishDraft]);

  // Snapshot the candidates for the duration of a gesture so a refetch cannot
  // unmount the element being dragged.
  const renderedProfiles = dragSession?.profiles ?? candidateProfiles;

  const chains = useMemo(() => {
    const byUid = new Map(renderedProfiles.map((item) => [item.uid, item]));
    const resolve = (uids: string[]) =>
      uids
        .map((uid) => byUid.get(uid))
        .filter((item): item is ScriptOrMergeProfile => Boolean(item));

    return {
      [ColumnType.Active]: resolve(draft[ColumnType.Active]),
      [ColumnType.Inactive]: resolve(draft[ColumnType.Inactive]),
    };
  }, [draft, renderedProfiles]);

  // Only the active column is persisted, so reordering the inactive one is not
  // a pending change.
  const isChanged = !sameOrder(
    draft[ColumnType.Active],
    serverChains[ColumnType.Active],
  );

  const blockTask = useBlockTask(taskKey, onApply);

  const saving = blockTask.isPending;
  const editable = dataReady && !hasDuplicates;
  const actionsDisabled = !editable || saving || dragSession !== null;

  const handleApply = async () => {
    if (!editable || savingRef.current || dragSessionRef.current) return;

    savingRef.current = true;

    try {
      const next = reconcileCandidates(draftRef.current, candidateUids);
      if (!sameColumns(next, draftRef.current)) publishDraft(next);

      await blockTask.execute(next[ColumnType.Active]);
    } finally {
      savingRef.current = false;
    }
  };

  const handleReset = () => {
    if (!editable || savingRef.current || dragSessionRef.current) return;

    adoptedActiveRef.current = serverChains[ColumnType.Active];
    publishDraft(serverChains);
  };

  const moveActive = useCallback(
    (uid: string, offset: -1 | 1) => {
      if (!editable || savingRef.current || dragSessionRef.current) return;
      const current = draftRef.current;
      const index = current.active.indexOf(uid);
      const nextIndex = index + offset;
      if (index < 0 || nextIndex < 0 || nextIndex >= current.active.length) {
        return;
      }
      const active = [...current.active];
      [active[index], active[nextIndex]] = [active[nextIndex]!, active[index]!];
      publishDraft({ ...current, active });
    },
    [editable, publishDraft],
  );

  const handleToggle = useCallback(
    (uid: string) => {
      if (!editable || savingRef.current || dragSessionRef.current) return;

      const current = draftRef.current;
      const source = current.active.includes(uid)
        ? ColumnType.Active
        : ColumnType.Inactive;
      const target =
        source === ColumnType.Active ? ColumnType.Inactive : ColumnType.Active;

      publishDraft({
        ...current,
        [source]: current[source].filter((item) => item !== uid),
        [target]: [...current[target], uid],
      });
    },
    [editable, publishDraft],
  );

  /** The source must be checked twice, before and after dnd-kit awaits. */
  const canDrag = useCallback(() => editable && !savingRef.current, [editable]);

  const beginDrag = useCallback(() => {
    const session = { columns: draftRef.current, profiles: candidateProfiles };
    dragSessionRef.current = session;
    setDragSession(session);
  }, [candidateProfiles]);

  const reorder = useCallback(
    (update: (current: ChainColumns) => ChainColumns) => {
      if (!dragSessionRef.current || savingRef.current) return;
      const next = update(draftRef.current);
      if (!sameColumns(next, draftRef.current)) publishDraft(next);
    },
    [publishDraft],
  );

  const endDrag = useCallback(
    (canceled: boolean) => {
      const session = dragSessionRef.current;
      if (canceled && session) publishDraft(session.columns);
      dragSessionRef.current = null;
      setDragSession(null);
    },
    [publishDraft],
  );

  return {
    dataReady,
    chains,
    isChanged,
    hasDuplicates,
    editable,
    saving,
    dragSession,
    actionsDisabled,
    handleApply,
    handleReset,
    moveActive,
    handleToggle,
    canDrag,
    beginDrag,
    reorder,
    endDrag,
  };
}

export type TransformChainDraft = ReturnType<typeof useTransformChainDraft>;
