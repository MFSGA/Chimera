import {
  useProfile,
  useRuntimeTransformDiagnostics,
  type LogSpan,
  type ProfileQueryResultItem,
} from '@chimera/interface';
import AddRounded from '~icons/material-symbols/add-rounded';
import ArrowDownwardRounded from '~icons/material-symbols/arrow-downward-rounded';
import ArrowUpwardRounded from '~icons/material-symbols/arrow-upward-rounded';
import CloseRounded from '~icons/material-symbols/close-rounded';
import { useMemo, useState, type ReactElement } from 'react';
import { useBlockTask } from '@/components/providers/block-task-provider';
import { Button } from '@/components/ui/button';
import {
  Card,
  CardContent,
  CardFooter,
  CardHeader,
} from '@/components/ui/card';
import {
  Modal,
  ModalContent,
  ModalTitle,
  ModalTrigger,
} from '@/components/ui/modal';
import { useLockFn } from '@/hooks/use-lock-fn';
import * as m from '@/paraglide/messages';
import { formatError } from '@/utils';
import { message } from '@/utils/notification';

const isTransformProfile = (profile: ProfileQueryResultItem) =>
  profile.type === 'merge' || profile.type === 'script';

const transformTypeLabel = (profile: ProfileQueryResultItem) => {
  if (profile.type === 'merge') return m.profile_merge_label();
  if (profile.type === 'script' && profile.script_type === 'lua') {
    return m.profile_lua_label();
  }
  return m.profile_javascript_label();
};

function TransformSummary({ profile }: { profile: ProfileQueryResultItem }) {
  return (
    <div className="min-w-0 flex-1">
      <p className="truncate text-sm font-medium">{profile.name}</p>
      <p className="truncate text-xs opacity-60">
        {transformTypeLabel(profile)}
      </p>
    </div>
  );
}

function TransformRuntimeLogs({ logs }: { logs: [LogSpan, string][] }) {
  if (logs.length === 0) return null;

  return (
    <div
      className="mt-2 max-h-28 space-y-1 overflow-y-auto border-t pt-2 pr-1"
      data-slot="transform-runtime-logs"
    >
      {logs.map(([span, text], index) => (
        <div
          key={`${span}-${index}-${text}`}
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
  );
}

type TransformChainEditorProps = {
  profile?: ProfileQueryResultItem;
  children: ReactElement;
};

export default function TransformChainEditor({
  profile,
  children,
}: TransformChainEditorProps) {
  const { query, setTransformChain, setGlobalTransformChain } = useProfile();
  const [open, setOpen] = useState(false);
  const [draft, setDraft] = useState<string[]>([]);
  const diagnostics = useRuntimeTransformDiagnostics(open);

  const transforms = useMemo(
    () => (query.data?.items ?? []).filter(isTransformProfile),
    [query.data?.items],
  );
  const transformByUid = useMemo(
    () => new Map(transforms.map((item) => [item.uid, item])),
    [transforms],
  );
  const sourceChain = useMemo(() => {
    if (!profile) return query.data?.global_transforms ?? [];
    if (profile.type !== 'local' && profile.type !== 'remote') return [];
    return profile.chain ?? [];
  }, [profile, query.data?.global_transforms]);
  const available = transforms.filter((item) => !draft.includes(item.uid));
  const runtimeLogsByUid = useMemo(() => {
    const current = diagnostics.data;
    if (!current) return new Map<string, [LogSpan, string][]>();
    const output = profile
      ? (current.output.scopes[profile.uid] ?? {})
      : current.output.global;
    return new Map(Object.entries(output));
  }, [diagnostics.data, profile]);
  const hasRuntimeLogs = useMemo(
    () => [...runtimeLogsByUid.values()].some((logs) => logs.length > 0),
    [runtimeLogsByUid],
  );
  const runtimeFailure = useMemo(() => {
    const failure = diagnostics.data?.failure;
    if (!failure) return null;
    if (profile) {
      return failure.scope_uid === profile.uid ? failure : null;
    }
    return failure.scope_uid === null ? failure : null;
  }, [diagnostics.data?.failure, profile]);

  const task = useBlockTask(
    `update-transform-chain-${profile?.uid ?? 'global'}`,
    async () => {
      try {
        const outcome = profile
          ? await setTransformChain.mutateAsync({
              uid: profile.uid,
              transforms: draft,
            })
          : await setGlobalTransformChain.mutateAsync(draft);
        await diagnostics.refetch();
        if (outcome?.status !== 'committed_degraded') {
          setOpen(false);
        }
      } catch (error) {
        await message(formatError(error), {
          title: m.common_error(),
          kind: 'error',
        });
      }
    },
  );
  const submit = useLockFn(task.execute);

  const move = (index: number, offset: -1 | 1) => {
    setDraft((current) => {
      const nextIndex = index + offset;
      if (nextIndex < 0 || nextIndex >= current.length) return current;
      const next = [...current];
      [next[index], next[nextIndex]] = [next[nextIndex], next[index]];
      return next;
    });
  };

  return (
    <Modal
      open={open}
      onOpenChange={(next) => {
        if (task.isPending) return;
        if (next) {
          setDraft([...sourceChain]);
          void diagnostics.refetch();
        }
        setOpen(next);
      }}
    >
      <ModalTrigger asChild>{children}</ModalTrigger>

      <ModalContent>
        <Card
          className="w-[34rem] max-w-[calc(100vw-2rem)]"
          data-slot="transform-chain-editor"
          data-chain-scope={profile ? 'profile' : 'global'}
        >
          <CardHeader>
            <ModalTitle>
              {profile
                ? m.profile_menu_proxy_chains()
                : m.profile_title_global_proxy_chains()}
            </ModalTitle>
            {profile && (
              <p className="truncate text-sm opacity-60">{profile.name}</p>
            )}
            {diagnostics.isFetching && diagnostics.data === undefined ? (
              <p
                className="text-xs opacity-50"
                data-slot="transform-runtime-diagnostics-loading"
              >
                {m.profile_chain_editor_diagnostics_loading()}
              </p>
            ) : diagnostics.isError ? (
              <p className="text-xs opacity-50">{m.common_error()}</p>
            ) : diagnostics.data === null ? (
              <p
                className="text-xs opacity-50"
                data-slot="transform-runtime-diagnostics-unavailable"
              >
                {m.profile_chain_editor_no_applied_runtime()}
              </p>
            ) : diagnostics.data ? (
              <div className="space-y-0.5">
                <p
                  className="text-xs opacity-50"
                  data-slot="transform-runtime-diagnostics"
                  data-runtime-revision={diagnostics.data.revision}
                >
                  {m.navbar_label_logs()} · r{diagnostics.data.revision}
                </p>
                {!hasRuntimeLogs && (
                  <p
                    className="text-xs opacity-40"
                    data-slot="transform-runtime-diagnostics-empty"
                  >
                    {m.profile_chain_editor_no_runtime_logs()}
                  </p>
                )}
              </div>
            ) : null}
          </CardHeader>

          <CardContent className="grid gap-4 md:grid-cols-2">
            <section className="min-w-0 space-y-2">
              <p className="text-sm font-semibold">
                {m.profile_chain_editor_active_column()}
              </p>
              <div className="max-h-72 space-y-2 overflow-y-auto pr-1">
                {draft.length === 0 ? (
                  <div className="rounded-2xl border border-dashed p-4 text-center text-sm opacity-50">
                    —
                  </div>
                ) : (
                  draft.map((uid, index) => {
                    const transform = transformByUid.get(uid);
                    if (!transform) {
                      return (
                        <div
                          key={uid}
                          className="flex items-center gap-2 rounded-2xl border p-2"
                          data-slot="transform-chain-active-item"
                          data-profile-uid={uid}
                        >
                          <p className="min-w-0 flex-1 truncate text-sm opacity-60">
                            {uid}
                          </p>
                          <Button
                            icon
                            className="size-8"
                            data-slot="transform-chain-remove"
                            onClick={() =>
                              setDraft((current) =>
                                current.filter((item) => item !== uid),
                              )
                            }
                          >
                            <CloseRounded className="size-4" />
                          </Button>
                        </div>
                      );
                    }

                    return (
                      <div
                        key={uid}
                        className="rounded-2xl border p-2"
                        data-slot="transform-chain-active-item"
                        data-profile-uid={uid}
                      >
                        <div className="flex items-center gap-1">
                          <TransformSummary profile={transform} />
                          <Button
                            icon
                            className="size-8"
                            disabled={index === 0}
                            data-slot="transform-chain-move-up"
                            onClick={() => move(index, -1)}
                          >
                            <ArrowUpwardRounded className="size-4" />
                          </Button>
                          <Button
                            icon
                            className="size-8"
                            disabled={index === draft.length - 1}
                            data-slot="transform-chain-move-down"
                            onClick={() => move(index, 1)}
                          >
                            <ArrowDownwardRounded className="size-4" />
                          </Button>
                          <Button
                            icon
                            className="size-8"
                            data-slot="transform-chain-remove"
                            onClick={() =>
                              setDraft((current) =>
                                current.filter((item) => item !== uid),
                              )
                            }
                          >
                            <CloseRounded className="size-4" />
                          </Button>
                        </div>
                        <TransformRuntimeLogs
                          logs={runtimeLogsByUid.get(uid) ?? []}
                        />
                        {runtimeFailure?.transform_uid === uid && (
                          <div
                            className="border-error/40 text-error mt-2 max-h-28 space-y-1 overflow-y-auto border-t pt-2 text-xs"
                            data-slot="transform-runtime-failure"
                            data-attempt-revision={
                              runtimeFailure.attempt_revision
                            }
                            data-script-type={runtimeFailure.script_type}
                          >
                            <p className="font-medium">
                              {m.common_error()} ·{' '}
                              {transformTypeLabel(transform)} · r
                              {runtimeFailure.attempt_revision}
                            </p>
                            <p className="font-mono break-words opacity-80">
                              {runtimeFailure.message}
                            </p>
                          </div>
                        )}
                      </div>
                    );
                  })
                )}
              </div>
            </section>

            <section className="min-w-0 space-y-2">
              <p className="text-sm font-semibold">
                {m.profile_chain_editor_inactive_column()}
              </p>
              <div className="max-h-72 space-y-2 overflow-y-auto pr-1">
                {available.length === 0 ? (
                  <div className="rounded-2xl border border-dashed p-4 text-center text-sm opacity-50">
                    —
                  </div>
                ) : (
                  available.map((transform) => (
                    <button
                      key={transform.uid}
                      type="button"
                      className="hover:bg-primary-container dark:hover:bg-surface-variant flex w-full cursor-pointer items-center gap-2 rounded-2xl border p-2 text-left transition-colors"
                      data-slot="transform-chain-inactive-item"
                      data-profile-uid={transform.uid}
                      onClick={() =>
                        setDraft((current) => [...current, transform.uid])
                      }
                    >
                      <TransformSummary profile={transform} />
                      <AddRounded className="size-5 shrink-0" />
                    </button>
                  ))
                )}
              </div>
            </section>
          </CardContent>

          <CardFooter className="gap-2">
            <Button
              onClick={submit}
              loading={task.isPending}
              disabled={task.isPending}
              data-slot="transform-chain-save"
            >
              {m.common_save()}
            </Button>
            <Button
              onClick={() => setOpen(false)}
              disabled={task.isPending}
              data-slot="transform-chain-cancel"
            >
              {m.common_cancel()}
            </Button>
          </CardFooter>
        </Card>
      </ModalContent>
    </Modal>
  );
}
