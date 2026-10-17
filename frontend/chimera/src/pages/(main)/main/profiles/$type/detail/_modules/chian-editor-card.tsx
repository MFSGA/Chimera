import {
  scopedTransformsOf,
  useProfile,
  useRuntimeTransformDiagnostics,
  type LogSpan,
  type ProfileDefinition_Deserialize,
} from '@chimera/interface';
import { cn } from '@chimera/ui';
import { move } from '@dnd-kit/helpers';
import { DragDropProvider } from '@dnd-kit/react';
import WarningRounded from '~icons/material-symbols/warning-rounded';
import { AnimatePresence, motion } from 'motion/react';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { Notice } from '@/components/base';
import { useBlockTask } from '@/components/providers/block-task-provider';
import { AnimatedItem } from '@/components/ui/animated-item';
import { Button } from '@/components/ui/button';
import {
  Card,
  CardContent,
  CardFooter,
  CardHeader,
} from '@/components/ui/card';
import { CircularProgress } from '@/components/ui/progress';
import * as m from '@/paraglide/messages';
import { formatError } from '@/utils';
import { WHOLE_ITEM_DRAG_SENSORS } from '@/utils/dnd-sensors';
import { message } from '@/utils/notification';
import {
  categoryProfiles,
  isChainProfile,
  type ConfigProfile,
  type TransformProfile,
} from '../../_modules/utils';
import {
  TransformChainColumn,
  type TransformChainLayout,
} from './transform-chain-column';

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

export default function ChianEditorCard({
  profile,
}: {
  profile: ConfigProfile;
}) {
  const { replaceDefinition } = useProfile();

  // The edited config item's own scoped transforms (File or Composition).
  const transforms = useMemo(() => scopedTransformsOf(profile), [profile]);

  // Keyed per profile so a draft cannot leak into another config whose baseline
  // happens to be identical.
  return (
    <TransformChainEditor
      key={profile.uid}
      layout="stacked"
      className="col-span-2 md:col-span-4"
      taskKey={`update-chain-${profile.uid}`}
      scopeUid={profile.uid}
      transforms={transforms}
      onApply={async (next) => {
        // Rebuild the config definition with the reordered scoped transforms,
        // preserving every other field, and replace it atomically.
        const definition: ProfileDefinition_Deserialize = {
          type: 'config',
          config: { ...profile.config, transforms: next },
        };

        await replaceDefinition.mutateAsync({ uid: profile.uid, definition });
      }}
    />
  );
}

/** Shared drag-and-drop editor for scoped and global transforms. */
export function TransformChainEditor({
  transforms: currentTransforms,
  taskKey,
  onApply,
  className,
  layout,
  scopeUid = null,
}: {
  /** The persisted chain, in order. */
  transforms: string[];
  taskKey: string;
  onApply: (transforms: string[]) => Promise<unknown>;
  className?: string;
  layout: TransformChainLayout;
  scopeUid?: string | null;
}) {
  const {
    query: { data: profiles },
  } = useProfile();
  const diagnostics = useRuntimeTransformDiagnostics(true);

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

  const blockTask = useBlockTask(taskKey, async (transforms: string[]) => {
    const previousAttemptRevision =
      diagnostics.data?.failure?.attempt_revision ??
      diagnostics.data?.revision ??
      0;
    try {
      await onApply(transforms);
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
      await message(`Update failed: \n ${formatError(error)}`, {
        title: m.common_error(),
        kind: 'error',
      });
    }
  });

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

  const loadingMessage = m.profile_chain_editor_apply_message();
  const runtimeLogsByUid = useMemo(() => {
    const current = diagnostics.data;
    if (!current) return new Map<string, [LogSpan, string][]>();
    const output = scopeUid
      ? (current.output.scopes[scopeUid] ?? {})
      : current.output.global;
    return new Map(Object.entries(output));
  }, [diagnostics.data, scopeUid]);
  const runtimeFailure = useMemo(() => {
    const failure = diagnostics.data?.failure;
    return failure?.scope_uid === scopeUid ? failure : null;
  }, [diagnostics.data?.failure, scopeUid]);

  // Stable so the memoized columns do not re-render on every drag-over.
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

  return (
    <Card
      data-slot="transform-chain-editor"
      data-ref-slot="profile-chain-editor"
      data-chain-scope={scopeUid ? 'profile' : 'global'}
      data-profile-uid={scopeUid ?? undefined}
      data-layout={layout}
      className={cn('relative', className)}
    >
      {layout === 'stacked' && (
        <CardHeader className="flex-col items-start gap-1">
          <h2 className="text-lg font-medium">
            {m.profile_chain_editor_title()}
          </h2>
          <p className="text-on-surface-variant text-sm">
            {m.profile_chain_editor_scoped_hint()}
          </p>
        </CardHeader>
      )}
      <CardContent className="pb-0">
        {diagnostics.isFetching && diagnostics.data === undefined ? (
          <p
            className="text-on-surface-variant text-xs"
            data-slot="transform-runtime-diagnostics-loading"
          >
            {m.profile_chain_editor_diagnostics_loading()}
          </p>
        ) : diagnostics.isError ? (
          <p className="text-on-surface-variant text-xs">{m.common_error()}</p>
        ) : diagnostics.data === null ? (
          <p
            className="text-on-surface-variant text-xs"
            data-slot="transform-runtime-diagnostics-unavailable"
          >
            {m.profile_chain_editor_no_applied_runtime()}
          </p>
        ) : diagnostics.data ? (
          <div className="space-y-1">
            <p
              data-slot="transform-runtime-diagnostics"
              data-runtime-revision={diagnostics.data.revision}
              className="text-on-surface-variant text-xs"
            >
              {m.navbar_label_logs()} · r{diagnostics.data.revision}
            </p>
            {!runtimeFailure &&
              ![...runtimeLogsByUid.values()].some(
                (logs) => logs.length > 0,
              ) && (
                <p
                  className="text-on-surface-variant text-xs"
                  data-slot="transform-runtime-diagnostics-empty"
                >
                  {m.profile_chain_editor_no_runtime_logs()}
                </p>
              )}
          </div>
        ) : null}
      </CardContent>
      <AnimatePresence initial={false}>
        {saving && (
          <motion.div
            data-slot="profile-chain-editor-saving-mask"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            className={cn(
              'bg-primary/10 absolute inset-0 z-50 backdrop-blur-3xl',
              'flex flex-col items-center justify-center gap-4',
            )}
          >
            <CircularProgress className="size-12" indeterminate />

            <p>{loadingMessage}</p>
          </motion.div>
        )}
      </AnimatePresence>

      {hasDuplicates && (
        <CardContent className="pb-0">
          <p role="alert" className="text-error flex items-start gap-2 text-sm">
            <WarningRounded className="mt-0.5 size-4 shrink-0" />
            {m.profile_chain_editor_duplicate_hint()}
          </p>
        </CardContent>
      )}

      <CardContent
        data-slot="profile-chain-editor-columns"
        className={cn(
          'grid',
          layout === 'grouped' && 'md:grid-cols-[1.25fr_1fr]',
        )}
      >
        <DragDropProvider
          sensors={WHOLE_ITEM_DRAG_SENSORS}
          onBeforeDragStart={(event) => {
            if (!editable || savingRef.current) event.preventDefault();
          }}
          onDragStart={(_event, manager) => {
            // dnd-kit awaits a render between beforedragstart and dragstart and
            // its continuation re-checks neither the source nor the editor, so
            // a save or a correction can have landed in that window.
            if (!editable || savingRef.current) {
              manager.actions.stop({ canceled: true });
              return;
            }

            const session = {
              columns: draftRef.current,
              profiles: candidateProfiles,
            };

            dragSessionRef.current = session;
            setDragSession(session);
          }}
          onDragOver={(event) => {
            // Synchronous and unconditional: the optimistic sorting plugin only
            // reads defaultPrevented in a microtask, so this reliably stops it
            // from relocating React-owned nodes into the other column.
            event.preventDefault();

            if (!dragSessionRef.current || savingRef.current) return;

            const next = move(draftRef.current, event);
            if (!sameColumns(next, draftRef.current)) publishDraft(next);
          }}
          onDragEnd={(event) => {
            const session = dragSessionRef.current;
            if (event.canceled && session) publishDraft(session.columns);

            dragSessionRef.current = null;
            setDragSession(null);
          }}
        >
          <TransformChainColumn
            profiles={chains.active}
            type={ColumnType.Active}
            saving={saving}
            readOnly={!editable && !dragSession}
            layout={layout}
            onToggle={handleToggle}
            onMove={moveActive}
            runtimeLogsByUid={runtimeLogsByUid}
            runtimeFailure={runtimeFailure}
            actionsDisabled={actionsDisabled}
          />

          <TransformChainColumn
            profiles={chains.inactive}
            type={ColumnType.Inactive}
            saving={saving}
            readOnly={!editable && !dragSession}
            layout={layout}
            onToggle={handleToggle}
            onMove={moveActive}
            runtimeLogsByUid={runtimeLogsByUid}
            runtimeFailure={runtimeFailure}
            actionsDisabled={actionsDisabled}
          />
        </DragDropProvider>
      </CardContent>

      <p
        data-slot="profile-chain-editor-drag-hint"
        className="text-on-surface-variant px-4 pb-4 text-xs"
      >
        {m.profile_chain_editor_drag_hint()}
      </p>

      <AnimatePresence>
        {dataReady && isChanged && (
          <AnimatedItem>
            <CardFooter className="gap-1">
              <Button
                variant="flat"
                className="flex items-center gap-2"
                onClick={handleApply}
                disabled={actionsDisabled}
                data-slot="transform-chain-save"
                data-ref-slot="common-apply"
              >
                {m.common_apply()}
              </Button>

              <Button
                onClick={handleReset}
                disabled={actionsDisabled}
                data-slot="transform-chain-reset"
              >
                {m.common_reset()}
              </Button>
            </CardFooter>
          </AnimatedItem>
        )}
      </AnimatePresence>
    </Card>
  );
}
