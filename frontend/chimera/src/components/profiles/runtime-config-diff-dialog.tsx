import {
  useProfile,
  useProfileContent,
  useRuntimeProfile,
} from '@chimera/interface';
import { BaseDialog, Button, cn } from '@chimera/ui';
import { useCreation } from 'ahooks';
import { useAtomValue } from 'jotai';
import { nanoid } from 'nanoid';
import { lazy, Suspense } from 'react';
import * as m from '@/paraglide/messages';
import { themeMode } from '@/store';

const MonacoDiffEditor = lazy(() => import('./profile-monaco-diff-viewer'));

export type RuntimeConfigDiffDialogProps = {
  open: boolean;
  onClose: () => void;
};

export default function RuntimeConfigDiffDialog({
  open,
  onClose,
}: RuntimeConfigDiffDialogProps) {
  const { query } = useProfile();

  const currentProfileUid = query.data?.current;

  const contentFn = useProfileContent(currentProfileUid || '');
  const runtimeProfile = useRuntimeProfile(open && !!currentProfileUid);

  const loaded =
    !!currentProfileUid &&
    contentFn.query.isSuccess &&
    runtimeProfile.isSuccess &&
    typeof contentFn.query.data === 'string' &&
    typeof runtimeProfile.data === 'string';
  const loading =
    query.isLoading || contentFn.query.isLoading || runtimeProfile.isLoading;
  const runtimeErrorMessage =
    runtimeProfile.error instanceof Error
      ? runtimeProfile.error.message
      : runtimeProfile.error
        ? String(runtimeProfile.error)
        : '';
  const error = runtimeProfile.isError
    ? runtimeProfile.error
    : contentFn.query.isError
      ? contentFn.query.error
      : query.error;
  const errorMessage =
    error instanceof Error ? error.message : error ? String(error) : '';
  const runtimeUnavailable =
    runtimeProfile.isError &&
    runtimeErrorMessage.includes('runtime configuration is unavailable');

  const retry = () => {
    void Promise.all([
      query.refetch(),
      currentProfileUid ? contentFn.query.refetch() : Promise.resolve(),
      currentProfileUid ? runtimeProfile.refetch() : Promise.resolve(),
    ]);
  };

  const mode = useAtomValue(themeMode);

  const originalModelPath = useCreation(() => `${nanoid()}.clash.yaml`, []);
  const modifiedModelPath = useCreation(() => `${nanoid()}.runtime.yaml`, []);

  if (!currentProfileUid && !query.isLoading && !query.isError) {
    return null;
  }

  return (
    <BaseDialog
      title={m.profile_runtime_config_title()}
      open={open}
      onClose={onClose}
    >
      <div className="xs:w-[95vw] h-full w-[80vw] px-4">
        <div
          className={cn(
            'items-center justify-between px-5 pb-2',
            loaded ? 'flex' : 'hidden',
          )}
        >
          <span className="text-base font-semibold">
            {m.profile_original_config_label()}
          </span>
          <span className="text-base font-semibold">
            {m.profile_runtime_config_label()}
          </span>
        </div>
        <div className="flex h-[75vh] w-full items-center justify-center">
          {loading && (
            <div className="text-on-surface-variant text-sm" role="status">
              {m.common_loading()}
            </div>
          )}
          {!loading &&
            (query.isError ||
              contentFn.query.isError ||
              runtimeProfile.isError) && (
              <div
                className="flex max-w-xl flex-col items-center gap-3 px-6 text-center"
                role="alert"
              >
                <span className="text-base font-semibold">
                  {runtimeProfile.isError
                    ? m.agent_finding_title_runtime_config_missing()
                    : m.settings_clash_core_manager_card_loading_error()}
                </span>
                <p className="text-on-surface-variant text-sm">
                  {runtimeUnavailable
                    ? m.agent_finding_runtime_config_missing()
                    : errorMessage}
                </p>
                <Button variant="stroked" onClick={retry}>
                  {m.dashboard_widget_config_retry()}
                </Button>
              </div>
            )}
          {loaded && (
            <Suspense
              fallback={<span className="text-sm">{m.common_loading()}</span>}
            >
              <MonacoDiffEditor
                language="yaml"
                theme={mode === 'light' ? 'vs' : 'vs-dark'}
                original={contentFn.query.data}
                originalModelPath={originalModelPath}
                modified={runtimeProfile.data}
                modifiedModelPath={modifiedModelPath}
                options={{
                  minimap: { enabled: false },
                  automaticLayout: true,
                  readOnly: true,
                }}
              />
            </Suspense>
          )}
          {!loading &&
            !loaded &&
            !query.isError &&
            !contentFn.query.isError &&
            !runtimeProfile.isError && (
              <div className="text-on-surface-variant text-sm" role="status">
                {m.agent_finding_runtime_config_missing()}
              </div>
            )}
        </div>
      </div>
    </BaseDialog>
  );
}
