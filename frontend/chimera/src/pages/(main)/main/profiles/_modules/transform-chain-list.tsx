import { commands, unwrapResult, type LogSpan } from '@chimera/interface';
import { AnimatedItem } from '@chimera/ui/animated-item';
import { Button } from '@chimera/ui/button';
import { cardVariants } from '@chimera/ui/card';
import { LinearProgress } from '@chimera/ui/progress';
import { cn } from '@chimera/utils';
import { move } from '@dnd-kit/helpers';
import { DragDropProvider } from '@dnd-kit/react';
import { useSortable } from '@dnd-kit/react/sortable';
import { Link } from '@tanstack/react-router';
import ArrowDownwardRounded from '~icons/material-symbols/arrow-downward-rounded';
import ArrowUpwardRounded from '~icons/material-symbols/arrow-upward-rounded';
import DeleteOutlineRounded from '~icons/material-symbols/delete-outline-rounded';
import EditSquareOutlineRounded from '~icons/material-symbols/edit-square-outline-rounded';
import ManageSearchRounded from '~icons/material-symbols/manage-search-rounded';
import PlaylistAddRounded from '~icons/material-symbols/playlist-add-rounded';
import WarningRounded from '~icons/material-symbols/warning-rounded';
import { AnimatePresence } from 'motion/react';
import { memo, type ReactNode } from 'react';
import * as m from '@/paraglide/messages';
import { WHOLE_ITEM_DRAG_SENSORS } from '@/utils/dnd-sensors';
import {
  sortableItemVariants,
  SortableOrder,
  useSortableTransition,
} from '../../_modules/sortable-item';
import { RowAction } from './compact-controls';
import { PROFILE_TYPE_NAMES } from './consts';
import { StepGroup, StepRowText, stepRowVariants } from './step-list';
import { TransformTypeBadge } from './transform-type-icon';
import type { TransformChainDraft } from './use-transform-chain-draft';
import { profileTypeOf, type TransformProfile } from './utils';

type ItemProps = {
  profile: TransformProfile;
  index: number;
  isLast: boolean;
  traceStep: string;
  unapplied: boolean;
  saving: boolean;
  actionsDisabled: boolean;
  runtimeLogs: [LogSpan, string][];
  runtimeFailure: TransformChainDraft['runtimeFailure'];
  onRemove: (uid: string) => void;
  onMove: (uid: string, offset: -1 | 1) => void;
};

function ItemContent({
  profile,
  index,
  isLast,
  traceStep,
  unapplied,
  saving,
  actionsDisabled,
  runtimeLogs,
  runtimeFailure,
  onRemove,
  onMove,
  itemRef,
  dragging,
  draggable,
}: ItemProps & {
  itemRef?: (element: Element | null) => void;
  dragging?: boolean;
  draggable?: boolean;
}) {
  const failure =
    runtimeFailure?.transform_uid === profile.uid ? runtimeFailure : null;

  return (
    <div className="min-w-0">
      <div
        ref={itemRef}
        data-slot="profile-chain-editor-item"
        data-uid={profile.uid}
        data-dragging={dragging || undefined}
        aria-label={
          draggable
            ? m.profile_chain_editor_drag_item({ name: profile.name })
            : undefined
        }
        className={cn(
          cardVariants(),
          sortableItemVariants({ draggable, dragging, disabled: saving }),
          'group/row flex min-h-14 min-w-0 items-center gap-3 py-1.5 pr-2 pl-3',
          unapplied && 'animate-row-enter motion-reduce:animate-none',
        )}
      >
        <SortableOrder active data-slot="profile-chain-editor-order">
          {index + 1}
        </SortableOrder>

        <TransformTypeBadge type={profileTypeOf(profile)} />

        <StepRowText
          title={profile.name}
          sub={
            <>
              {PROFILE_TYPE_NAMES[profileTypeOf(profile)]}
              {unapplied && (
                <span
                  data-slot="profile-chain-editor-unapplied"
                  className="text-primary"
                >
                  {' · '}
                  {m.profile_chain_editor_unapplied()}
                </span>
              )}
            </>
          }
        />

        <div
          data-slot="profile-chain-editor-row-actions"
          className={cn(
            'flex shrink-0 items-center',
            'transition-opacity motion-reduce:transition-none',
            '[@media(hover:hover)]:opacity-0',
            'group-focus-within/row:opacity-100 group-hover/row:opacity-100',
          )}
        >
          <RowAction
            label={m.profile_chain_editor_open_editor()}
            data-slot="profile-chain-editor-open-editor"
            onClick={async () => {
              unwrapResult(
                await commands.createEditorWindow('profile', profile.uid),
              );
            }}
          >
            <EditSquareOutlineRounded className="size-5" aria-hidden />
          </RowAction>

          <RowAction
            asChild
            label={m.profile_chain_editor_trace()}
            data-slot="profile-chain-editor-trace"
          >
            <Link to="/main/profiles/inspect" search={{ step: traceStep }}>
              <ManageSearchRounded className="size-5" aria-hidden />
            </Link>
          </RowAction>

          <RowAction
            label={m.profile_chain_editor_move_up({ name: profile.name })}
            disabled={actionsDisabled || index === 0}
            data-slot="transform-chain-move-up"
            className="size-8"
            onClick={() => onMove(profile.uid, -1)}
          >
            <ArrowUpwardRounded className="size-4" aria-hidden />
          </RowAction>

          <RowAction
            label={m.profile_chain_editor_move_down({ name: profile.name })}
            disabled={actionsDisabled || isLast}
            data-slot="transform-chain-move-down"
            className="size-8"
            onClick={() => onMove(profile.uid, 1)}
          >
            <ArrowDownwardRounded className="size-4" aria-hidden />
          </RowAction>

          <RowAction
            label={m.profile_chain_editor_remove()}
            disabled={actionsDisabled}
            data-slot="transform-chain-remove"
            data-ref-slot="profile-chain-editor-remove"
            onClick={() => onRemove(profile.uid)}
          >
            <DeleteOutlineRounded className="size-5" aria-hidden />
          </RowAction>
        </div>
      </div>

      {runtimeLogs.length > 0 && (
        <div
          className="mt-2 max-h-28 space-y-1 overflow-y-auto border-t pt-2 pr-1"
          data-slot="transform-runtime-logs"
        >
          {runtimeLogs.map(([span, text], logIndex) => (
            <div
              key={span + '-' + logIndex + '-' + text}
              className="flex min-w-0 gap-2 text-xs"
              data-slot="transform-runtime-log"
              data-log-span={span}
            >
              <span className="w-10 shrink-0 font-mono uppercase opacity-50">
                {span}
              </span>
              <span className="min-w-0 break-words opacity-80">{text}</span>
            </div>
          ))}
        </div>
      )}

      {failure && (
        <div
          className="border-error/40 text-error mt-2 max-h-28 space-y-1 overflow-y-auto border-t pt-2 text-xs"
          data-slot="transform-runtime-failure"
          data-attempt-revision={failure.attempt_revision}
          data-transform-type={profile.type}
          data-script-type={failure.script_type ?? undefined}
        >
          <p className="font-medium">
            {m.common_error()} · {PROFILE_TYPE_NAMES[profileTypeOf(profile)]} ·
            r{failure.attempt_revision}
          </p>
          <p className="font-mono break-words opacity-80">{failure.message}</p>
        </div>
      )}
    </div>
  );
}

const Item = memo(function Item(props: ItemProps) {
  const { ref, isDragging } = useSortable({
    id: props.profile.uid,
    index: props.index,
    disabled: props.saving,
    transition: useSortableTransition(),
  });

  return (
    <ItemContent
      {...props}
      itemRef={ref}
      dragging={isDragging}
      draggable={!props.saving}
    />
  );
});

/** Count of enabled transforms. */
export function TransformChainHeader({
  chain,
}: {
  chain: TransformChainDraft;
}) {
  return (
    <p
      data-slot="profile-chain-editor-header"
      className="text-on-surface-variant px-3 text-sm"
    >
      {m.profile_chain_editor_enabled_count({ count: chain.enabled.length })}
    </p>
  );
}

function summaryOf({
  added,
  removed,
  reordered,
}: TransformChainDraft['changes']) {
  return [
    added > 0 && m.profile_chain_editor_summary_added({ count: added }),
    removed > 0 && m.profile_chain_editor_summary_removed({ count: removed }),
    reordered && m.profile_chain_editor_summary_reordered(),
  ]
    .filter(Boolean)
    .join(' · ');
}

/** Appears once the draft differs from the persisted chain. */
export function TransformChainBar({ chain }: { chain: TransformChainDraft }) {
  const summary = summaryOf(chain.changes);

  return (
    <AnimatePresence>
      {chain.dataReady && chain.isChanged && (
        <AnimatedItem
          data-slot="profile-chain-editor-bar"
          className="bg-surface sticky bottom-0 z-10 rounded-full"
        >
          <div className="flex items-center justify-end gap-1 p-2">
            {summary && (
              <p
                data-slot="profile-chain-editor-summary"
                className="text-on-surface-variant min-w-0 flex-1 truncate pl-3 text-xs"
              >
                {summary}
              </p>
            )}

            <Button
              onClick={chain.handleReset}
              disabled={chain.actionsDisabled}
              data-slot="transform-chain-reset"
            >
              {m.common_reset()}
            </Button>

            <Button
              variant="flat"
              onClick={chain.handleApply}
              disabled={chain.actionsDisabled}
              data-slot="transform-chain-save"
              data-ref-slot="profile-chain-editor-apply"
            >
              {m.common_apply()}
            </Button>
          </div>
        </AnimatedItem>
      )}
    </AnimatePresence>
  );
}

/** Compact applied-revision status; detailed steps and output live in Inspect. */
export function TransformRuntimeDiagnostics({
  chain,
}: {
  chain: TransformChainDraft;
}) {
  const hasLogs = [...chain.runtimeLogsByUid.values()].some(
    (logs) => logs.length > 0,
  );

  if (chain.diagnostics.isFetching && chain.diagnostics.data === undefined) {
    return (
      <p
        className="text-on-surface-variant text-xs"
        data-slot="transform-runtime-diagnostics-loading"
      >
        {m.profile_chain_editor_diagnostics_loading()}
      </p>
    );
  }

  if (chain.diagnostics.isError) {
    return (
      <p className="text-on-surface-variant text-xs">{m.common_error()}</p>
    );
  }

  if (chain.diagnostics.data === null) {
    return (
      <p
        className="text-on-surface-variant text-xs"
        data-slot="transform-runtime-diagnostics-unavailable"
      >
        {m.profile_chain_editor_no_applied_runtime()}
      </p>
    );
  }

  if (!chain.diagnostics.data) return null;

  return (
    <div className="space-y-1">
      <p
        data-slot="transform-runtime-diagnostics"
        data-runtime-revision={chain.diagnostics.data.revision}
        className="text-on-surface-variant text-xs"
      >
        {m.navbar_label_logs()} · r{chain.diagnostics.data.revision}
      </p>
      {!chain.runtimeFailure && !hasLogs && (
        <p
          className="text-on-surface-variant text-xs"
          data-slot="transform-runtime-diagnostics-empty"
        >
          {m.profile_chain_editor_no_runtime_logs()}
        </p>
      )}
    </div>
  );
}

/**
 * The sortable list of one enabled transform chain. The library and the
 * diagnostics summary are separate siblings owned by the caller.
 */
export function TransformChain({
  chain,
  traceStepOf,
  empty,
  header,
  footer,
  className,
}: {
  chain: TransformChainDraft;
  traceStepOf: (uid: string, index: number) => string;
  empty?: { message: string; hint: string };
  header?: ReactNode;
  footer?: ReactNode;
  className?: string;
}) {
  const readOnly = !chain.editable && !chain.dragging;

  return (
    <section
      data-slot="profile-chain-editor"
      aria-busy={chain.saving}
      className={cn('relative flex min-w-0 flex-col gap-3', className)}
    >
      {header}

      {chain.saving && (
        <LinearProgress
          indeterminate
          className="h-1"
          aria-label={m.profile_chain_editor_apply_message()}
        />
      )}

      {chain.hasDuplicates && (
        <p role="alert" className="text-error flex items-start gap-2 text-sm">
          <WarningRounded className="mt-0.5 size-4 shrink-0" aria-hidden />
          {m.profile_chain_editor_duplicate_hint()}
        </p>
      )}

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
        {chain.enabled.length === 0 ? (
          <StepGroup>
            <div
              data-slot="profile-chain-editor-empty"
              className={stepRowVariants()}
            >
              <span className="bg-secondary-container text-on-secondary-container grid size-9 shrink-0 place-items-center rounded-full">
                <PlaylistAddRounded className="size-5" aria-hidden />
              </span>
              <StepRowText
                dim
                title={empty?.message ?? m.profile_chain_editor_empty()}
                sub={empty?.hint ?? m.profile_chain_editor_empty_hint()}
              />
            </div>
          </StepGroup>
        ) : (
          <StepGroup data-slot="profile-chain-editor-items">
            {chain.enabled.map((profile, index) => {
              const itemProps: ItemProps = {
                profile,
                index,
                isLast: index === chain.enabled.length - 1,
                traceStep: traceStepOf(profile.uid, index),
                unapplied: chain.isUnapplied(profile.uid),
                saving: chain.saving,
                actionsDisabled: chain.actionsDisabled,
                runtimeLogs: chain.runtimeLogsByUid.get(profile.uid) ?? [],
                runtimeFailure: chain.runtimeFailure,
                onRemove: chain.handleRemove,
                onMove: chain.moveActive,
              };

              return (
                <div
                  key={readOnly ? profile.uid + '-' + index : profile.uid}
                  data-slot="transform-chain-active-item"
                  data-profile-uid={profile.uid}
                  data-uid={profile.uid}
                  className="min-w-0"
                >
                  {readOnly ? (
                    <ItemContent {...itemProps} saving actionsDisabled />
                  ) : (
                    <Item key={profile.uid} {...itemProps} />
                  )}
                </div>
              );
            })}
          </StepGroup>
        )}
      </DragDropProvider>

      {footer}
    </section>
  );
}
