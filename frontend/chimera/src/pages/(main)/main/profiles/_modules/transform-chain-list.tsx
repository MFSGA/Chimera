import {
  useRuntimeTransformDiagnostics,
  type LogSpan,
} from '@chimera/interface';
import { cn } from '@chimera/ui';
import { AnimatedItem } from '@chimera/ui/animated-item';
import { Button } from '@chimera/ui/button';
import { Card, CardContent, CardFooter, CardHeader } from '@chimera/ui/card';
import { CircularProgress } from '@chimera/ui/progress';
import { move } from '@dnd-kit/helpers';
import { DragDropProvider } from '@dnd-kit/react';
import WarningRounded from '~icons/material-symbols/warning-rounded';
import { AnimatePresence, motion } from 'motion/react';
import { useMemo } from 'react';
import { Notice } from '@/components/base';
import * as m from '@/paraglide/messages';
import { formatError } from '@/utils';
import { WHOLE_ITEM_DRAG_SENSORS } from '@/utils/dnd-sensors';
import { message } from '@/utils/notification';
import {
  TransformChainColumn,
  type TransformChainLayout,
} from './transform-chain-column';
import { useTransformChainDraft } from './use-transform-chain-draft';

export function TransformChainEditor({
  transforms: currentTransforms,
  taskKey,
  onApply,
  className,
  layout,
  scopeUid = null,
  showTitle = true,
}: {
  /** The persisted chain, in order. */
  transforms: string[];
  taskKey: string;
  onApply: (transforms: string[]) => Promise<unknown>;
  className?: string;
  layout: TransformChainLayout;
  scopeUid?: string | null;
  /** The section header replaces the editor title on the detail page. */
  showTitle?: boolean;
}) {
  const diagnostics = useRuntimeTransformDiagnostics(true);

  const applyWithDiagnostics = async (transforms: string[]) => {
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
  };

  const chain = useTransformChainDraft({
    transforms: currentTransforms,
    taskKey,
    onApply: applyWithDiagnostics,
    layout,
  });
  const {
    dataReady,
    chains,
    hasDuplicates,
    editable,
    saving,
    dragSession,
    actionsDisabled,
    isChanged,
    handleApply,
    handleReset,
    moveActive,
    handleToggle,
  } = chain;

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
  return (
    <Card
      data-slot="transform-chain-editor"
      data-ref-slot="profile-chain-editor"
      data-chain-scope={scopeUid ? 'profile' : 'global'}
      data-profile-uid={scopeUid ?? undefined}
      data-layout={layout}
      className={cn('relative', className)}
    >
      {layout === 'stacked' && showTitle && (
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
            if (!chain.canDrag()) event.preventDefault();
          }}
          onDragStart={(_event, manager) => {
            if (!chain.canDrag()) {
              manager.actions.stop({ canceled: true });
              return;
            }
            chain.beginDrag();
          }}
          onDragOver={(event) => {
            event.preventDefault();
            chain.reorder((current) => move(current, event));
          }}
          onDragEnd={(event) => chain.endDrag(event.canceled)}
        >
          <TransformChainColumn
            profiles={chains.active}
            type="active"
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
            type="inactive"
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
