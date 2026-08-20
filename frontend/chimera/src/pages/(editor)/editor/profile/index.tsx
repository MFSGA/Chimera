import {
  useProfile,
  useProfileContent,
  useRuntimeTransformDiagnostics,
} from '@chimera/interface';
import { createFileRoute } from '@tanstack/react-router';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import { ask } from '@tauri-apps/plugin-dialog';
import type { editor } from 'monaco-editor';
import { useCallback, useEffect, useRef, useState } from 'react';
import ProfileMonacoViewer from '@/components/profiles/profile-monaco-viewer';
import { useBlockTask } from '@/components/providers/block-task-provider';
import { Button } from '@/components/ui/button';
import TextMarquee from '@/components/ui/text-marquee';
import { useLockFn } from '@/hooks/use-lock-fn';
import * as m from '@/paraglide/messages';
import { formatError } from '@/utils';
import { message } from '@/utils/notification';
import Header from '../_modules/header';

const currentWindow = getCurrentWebviewWindow();

export const Route = createFileRoute('/(editor)/editor/profile/')({
  component: RouteComponent,
  validateSearch: (search): { uid: string } => ({
    uid: typeof search.uid === 'string' ? search.uid : '',
  }),
});

function RouteComponent() {
  const { uid } = Route.useSearch();
  const { query: profiles } = useProfile();
  const content = useProfileContent(uid);
  const profile = profiles.data?.items.find((item) => item.uid === uid);
  const isTransform = profile?.type === 'merge' || profile?.type === 'script';
  const diagnostics = useRuntimeTransformDiagnostics(isTransform);
  const runtimeFailure =
    diagnostics.data?.failure?.transform_uid === uid
      ? diagnostics.data.failure
      : null;
  const readOnly = profile?.type === 'remote';
  const language =
    profile?.type === 'script'
      ? profile.script_type === 'lua'
        ? 'lua'
        : 'javascript'
      : 'yaml';
  const schemaType = profile?.type === 'merge' ? 'merge' : 'clash';
  const markers = useRef<editor.IMarker[]>([]);
  const skipCloseGuard = useRef(false);
  const loadedContent = useRef<{ uid: string; value: string } | undefined>(
    undefined,
  );
  const [editorValue, setEditorValue] = useState<string>();

  useEffect(() => {
    const nextValue = content.query.data;
    if (typeof nextValue !== 'string') return;

    const previous = loadedContent.current;
    loadedContent.current = { uid, value: nextValue };
    setEditorValue((current) => {
      const switchedProfile = previous?.uid !== uid;
      const remainedUnedited = current === previous?.value;
      return current === undefined || switchedProfile || remainedUnedited
        ? nextValue
        : current;
    });
  }, [content.query.data, uid]);

  const dirty =
    editorValue !== undefined &&
    content.query.data !== undefined &&
    editorValue !== content.query.data;

  const confirmDirtyClose = useCallback(async () => {
    if (!dirty) return true;
    return ask(m.editor_before_close_message(), { kind: 'warning' });
  }, [dirty]);

  const beforeClose = useCallback(async () => {
    const accepted = await confirmDirtyClose();
    if (accepted) skipCloseGuard.current = true;
    return accepted;
  }, [confirmDirtyClose]);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;

    void currentWindow
      .onCloseRequested(async (event) => {
        if (skipCloseGuard.current) {
          skipCloseGuard.current = false;
          return;
        }
        if (!(await confirmDirtyClose())) event.preventDefault();
      })
      .then((dispose) => {
        if (disposed) dispose();
        else unlisten = dispose;
      });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [confirmDirtyClose]);

  const saveTask = useBlockTask(`save-profile-content-${uid}`, async () => {
    if (readOnly || editorValue === undefined) return;
    if (markers.current.some((marker) => marker.severity === 8)) {
      await message(m.editor_validate_error_message(), {
        title: m.common_error(),
        kind: 'error',
      });
      return;
    }

    try {
      const outcome = await content.upsert.mutateAsync(editorValue);
      if (outcome?.status === 'committed_degraded') {
        await diagnostics.refetch();
        return;
      }
      skipCloseGuard.current = true;
      await currentWindow.close();
    } catch (error) {
      await message(formatError(error), {
        title: m.common_error(),
        kind: 'error',
      });
    }
  });
  const save = useLockFn(saveTask.execute);

  const cancel = useLockFn(async () => {
    if (!(await confirmDirtyClose())) return;
    skipCloseGuard.current = true;
    await currentWindow.close();
  });

  const reset = useCallback(() => {
    setEditorValue(content.query.data ?? '');
  }, [content.query.data]);

  const loading = profiles.isLoading || content.query.isLoading;
  const error = profiles.error ?? content.query.error;

  return (
    <>
      <Header beforeClose={beforeClose} />

      {loading ? (
        <div className="text-on-surface-variant grid min-h-0 flex-1 place-items-center text-sm">
          {m.common_loading()}
        </div>
      ) : !profile || error ? (
        <div className="text-error grid min-h-0 flex-1 place-items-center p-4 text-sm">
          {error ? formatError(error) : `Profile not found: ${uid}`}
        </div>
      ) : (
        <>
          <div className="bg-primary-container dark:bg-on-primary flex h-12 shrink-0 items-center gap-2 px-3">
            <TextMarquee className="min-w-0 flex-1 text-sm font-medium">
              {profile.name}.{profile.file.split('.').pop() ?? 'yaml'}
            </TextMarquee>
            {readOnly && (
              <span className="bg-surface rounded-full px-3 py-1 text-xs font-bold">
                {m.editor_read_only_chip()}
              </span>
            )}
          </div>

          <div className="min-h-0 flex-1">
            <ProfileMonacoViewer
              className="h-full w-full"
              value={editorValue}
              language={language}
              schemaType={language === 'yaml' ? schemaType : undefined}
              readonly={readOnly}
              onChange={setEditorValue}
              onValidate={(nextMarkers) => {
                markers.current = nextMarkers;
              }}
            />
          </div>

          {runtimeFailure && (
            <div
              className="border-error/40 text-error bg-error-container/20 mx-3 mb-2 max-h-28 shrink-0 overflow-y-auto rounded-xl border px-3 py-2 text-xs"
              data-slot="profile-editor-runtime-failure"
              data-attempt-revision={runtimeFailure.attempt_revision}
              data-script-type={runtimeFailure.script_type ?? undefined}
            >
              <p className="font-medium">
                {m.common_error()} ·{' '}
                {profile.type === 'merge'
                  ? m.profile_merge_label()
                  : profile.type === 'script' && profile.script_type === 'lua'
                    ? m.profile_lua_label()
                    : m.profile_javascript_label()}{' '}
                · r{runtimeFailure.attempt_revision}
              </p>
              <p className="mt-1 font-mono break-words opacity-80">
                {runtimeFailure.message}
              </p>
            </div>
          )}

          <div className="bg-background flex h-12 shrink-0 items-center gap-2 px-3">
            <Button disabled={!dirty || readOnly} onClick={reset}>
              {m.common_reset()}
            </Button>
            <div className="flex-1" />
            <Button
              data-slot="profile-editor-cancel"
              onClick={() => void cancel()}
            >
              {m.common_cancel()}
            </Button>
            <Button
              variant="flat"
              disabled={!dirty || readOnly}
              loading={saveTask.isPending}
              data-slot="profile-editor-save"
              onClick={() => void save()}
            >
              {m.common_save()}
            </Button>
          </div>
        </>
      )}
    </>
  );
}
