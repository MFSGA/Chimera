import {
  useProfile,
  useRuntimeTransformDiagnostics,
  type LogSpan,
  type RuntimeTransformFailureDiagnostics,
} from '@chimera/interface';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { Notice } from '@/components/base';
import { useBlockTask } from '@/components/providers/block-task-provider';
import * as m from '@/paraglide/messages';
import { formatError } from '@/utils';
import { message } from '@/utils/notification';
import {
  changesOf,
  retainKnownDraft,
  sameOrder,
  serverChainOf,
} from './transform-chain-model';
import { isChainProfile, type TransformProfile } from './utils';

type DragSession = {
  uids: string[];
  profiles: TransformProfile[];
};

/**
 * Draft of an enabled transform chain. The persisted order wins until the
 * user edits it, and uids without a matching transform profile are dropped.
 */
export function useTransformChainDraft({
  transforms: currentTransforms,
  taskKey,
  onApply,
  scopeUid = null,
}: {
  /** The persisted chain, in order. */
  transforms: string[];
  taskKey: string;
  onApply: (transforms: string[]) => Promise<unknown>;
  /** null for the global chain; otherwise the config whose chain is edited. */
  scopeUid?: string | null;
}) {
  const {
    query: { data: profiles },
  } = useProfile();

  const dataReady = profiles != null;
  const candidates = useMemo(
    () => (profiles?.items ?? []).filter(isChainProfile),
    [profiles?.items],
  );
  const candidateUids = useMemo(
    () => candidates.map((item) => item.uid),
    [candidates],
  );

  const serverEnabled = useMemo(
    () => serverChainOf(currentTransforms, candidateUids),
    [currentTransforms, candidateUids],
  );

  const [draft, setDraft] = useState<string[]>(serverEnabled);

  // A duplicate in the persisted chain or draft would register the same
  // sortable id more than once. Keep editing disabled until the data is fixed.
  const hasDuplicates =
    new Set(currentTransforms).size !== currentTransforms.length ||
    new Set(draft).size !== draft.length;

  const draftRef = useRef(draft);
  const adoptedRef = useRef<string[] | null>(null);
  const dragSessionRef = useRef<DragSession | null>(null);
  const [dragSession, setDragSession] = useState<DragSession | null>(null);
  const savingRef = useRef(false);

  const publishDraft = useCallback((next: string[]) => {
    draftRef.current = next;
    setDraft(next);
  }, []);

  useEffect(() => {
    if (!dataReady || dragSessionRef.current) return;

    const serverChanged =
      adoptedRef.current === null ||
      !sameOrder(adoptedRef.current, serverEnabled);

    // A changed chain is the server's call; a changed candidate list only
    // removes entries and must not discard the user's draft order.
    const next = serverChanged
      ? serverEnabled
      : retainKnownDraft(draftRef.current, candidateUids);

    adoptedRef.current = serverEnabled;
    if (!sameOrder(next, draftRef.current)) publishDraft(next);
  }, [serverEnabled, candidateUids, dragSession, dataReady, publishDraft]);

  // Freeze the candidate snapshot during a drag so a refetch cannot unmount
  // the element being dragged.
  const renderedProfiles = dragSession?.profiles ?? candidates;
  const enabled = useMemo(() => {
    const byUid = new Map(renderedProfiles.map((item) => [item.uid, item]));
    return draft
      .map((uid) => byUid.get(uid))
      .filter((item): item is TransformProfile => Boolean(item));
  }, [draft, renderedProfiles]);

  const isChanged = !sameOrder(draft, serverEnabled);
  const appliedUids = useMemo(() => new Set(serverEnabled), [serverEnabled]);
  const changes = useMemo(
    () => changesOf(draft, serverEnabled),
    [draft, serverEnabled],
  );

  const isUnapplied = useCallback(
    (uid: string) => !appliedUids.has(uid),
    [appliedUids],
  );

  const diagnostics = useRuntimeTransformDiagnostics(true);
  const applyWithDiagnostics = useCallback(
    async (next: string[]) => {
      const previousAttemptRevision =
        diagnostics.data?.failure?.attempt_revision ??
        diagnostics.data?.revision ??
        0;
      try {
        await onApply(next);
        await diagnostics.refetch();
      } catch (error) {
        const refreshed = await diagnostics.refetch();
        const failure = refreshed.data?.failure;
        if (
          failure &&
          failure.scope_uid === scopeUid &&
          failure.attempt_revision > previousAttemptRevision
        ) {
          Notice.error(formatError(error), 5000);
          return;
        }
        await message(
          m.profile_chain_editor_update_failed({ error: formatError(error) }),
          {
            title: m.profile_chain_editor_update_failed_title(),
            kind: 'error',
          },
        );
      }
    },
    [
      diagnostics.data?.failure?.attempt_revision,
      diagnostics.data?.revision,
      diagnostics.refetch,
      onApply,
      scopeUid,
    ],
  );

  const blockTask = useBlockTask(taskKey, applyWithDiagnostics);
  const saving = blockTask.isPending;
  const editable = dataReady && !hasDuplicates;
  const dragging = dragSession !== null;
  const actionsDisabled = !editable || saving || dragging;

  const canEdit = useCallback(
    () => editable && !savingRef.current && !dragSessionRef.current,
    [editable],
  );

  const handleAdd = useCallback(
    (uid: string) => {
      if (!canEdit() || draftRef.current.includes(uid)) return;
      publishDraft([...draftRef.current, uid]);
    },
    [canEdit, publishDraft],
  );

  const handleRemove = useCallback(
    (uid: string) => {
      if (!canEdit()) return;
      publishDraft(draftRef.current.filter((item) => item !== uid));
    },
    [canEdit, publishDraft],
  );

  const moveActive = useCallback(
    (uid: string, offset: -1 | 1) => {
      if (!canEdit()) return;
      const current = draftRef.current;
      const index = current.indexOf(uid);
      const nextIndex = index + offset;
      if (index < 0 || nextIndex < 0 || nextIndex >= current.length) return;
      const next = [...current];
      [next[index], next[nextIndex]] = [next[nextIndex]!, next[index]!];
      publishDraft(next);
    },
    [canEdit, publishDraft],
  );

  const handleApply = async () => {
    if (!canEdit()) return;
    savingRef.current = true;
    try {
      const next = retainKnownDraft(draftRef.current, candidateUids);
      if (!sameOrder(next, draftRef.current)) publishDraft(next);
      await blockTask.execute(next);
    } finally {
      savingRef.current = false;
    }
  };

  const handleReset = () => {
    if (!canEdit()) return;
    adoptedRef.current = serverEnabled;
    publishDraft(serverEnabled);
  };

  const canDrag = useCallback(() => editable && !savingRef.current, [editable]);

  const beginDrag = useCallback(() => {
    const session = { uids: draftRef.current, profiles: candidates };
    dragSessionRef.current = session;
    setDragSession(session);
  }, [candidates]);

  const reorder = useCallback(
    (update: (current: string[]) => string[]) => {
      if (!dragSessionRef.current || savingRef.current) return;
      const next = update(draftRef.current);
      if (!sameOrder(next, draftRef.current)) publishDraft(next);
    },
    [publishDraft],
  );

  const endDrag = useCallback(
    (canceled: boolean) => {
      const session = dragSessionRef.current;
      if (canceled && session) publishDraft(session.uids);
      dragSessionRef.current = null;
      setDragSession(null);
    },
    [publishDraft],
  );

  const runtimeLogsByUid = useMemo(() => {
    const current = diagnostics.data;
    if (!current) return new Map<string, [LogSpan, string][]>();
    const output = scopeUid
      ? (current.output.scopes[scopeUid] ?? {})
      : current.output.global;
    return new Map(Object.entries(output));
  }, [diagnostics.data, scopeUid]);

  const runtimeFailure =
    useMemo<RuntimeTransformFailureDiagnostics | null>(() => {
      const failure = diagnostics.data?.failure;
      return failure?.scope_uid === scopeUid ? failure : null;
    }, [diagnostics.data?.failure, scopeUid]);

  return {
    dataReady,
    enabled,
    candidates,
    isChanged,
    changes,
    isUnapplied,
    hasDuplicates,
    editable,
    saving,
    dragging,
    actionsDisabled,
    handleAdd,
    handleRemove,
    moveActive,
    handleApply,
    handleReset,
    canDrag,
    beginDrag,
    reorder,
    endDrag,
    diagnostics,
    runtimeLogsByUid,
    runtimeFailure,
  };
}

export type TransformChainDraft = ReturnType<typeof useTransformChainDraft>;
