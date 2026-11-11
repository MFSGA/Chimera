import { commands, openThat, type AppUpdateSnapshot } from '@chimera/interface';
import { BaseDialog, BaseDialogProps, cn } from '@chimera/ui';
import { Button, LinearProgress } from '@mui/material';
import { useLockFn } from 'ahooks';
import dayjs from 'dayjs';
import { useSetAtom } from 'jotai';
import AppUpdateChangelog from '@/components/updater/app-update-changelog';
import * as m from '@/paraglide/messages';
import { UpdaterIgnoredAtom } from '@/store/updater';
import { formatError } from '@/utils';
import { message } from '@/utils/notification';
import { useChimeraUpdate } from '../providers/chimera-update-provider';
import styles from './updater-dialog.module.scss';

export interface UpdaterDialogProps extends Omit<BaseDialogProps, 'title'> {
  update: AppUpdateSnapshot;
}

export default function UpdaterDialog({
  open,
  update,
  onClose,
  ...others
}: UpdaterDialogProps) {
  const setUpdaterIgnore = useSetAtom(UpdaterIgnoredAtom);
  const { currentVersion, downloadUpdate, cancelDownload, installUpdate } =
    useChimeraUpdate();
  const release = update.release;
  const pending =
    update.phase === 'downloading' ||
    update.phase === 'verifying' ||
    update.phase === 'cancelling' ||
    update.phase === 'installing';
  const progress =
    update.downloaded && update.total
      ? (update.downloaded / update.total) * 100
      : 0;

  const handleUpdate = useLockFn(async () => {
    try {
      if (update.phase === 'ready') await installUpdate();
      else await downloadUpdate();
    } catch (error) {
      message(formatError(error), { kind: 'error', title: m.common_error() });
    }
  });

  const handleCancel = useLockFn(async () => {
    try {
      await cancelDownload();
    } catch (error) {
      message(formatError(error), { kind: 'error', title: m.common_error() });
    }
  });

  return (
    <BaseDialog
      {...others}
      title={m.updater_update_available()}
      open={open}
      onClose={() => {
        if (release) setUpdaterIgnore(release.version);
        onClose?.();
      }}
      onOk={handleUpdate}
      loading={pending}
      close={m.common_close()}
      closeTestId="updater-dialog-close"
      ok={m.common_update()}
      divider
    >
      <div
        className={cn(
          'xs:min-w-[90vw] sm:min-w-[55vw] md:min-w-[33.3vw]',
          styles.UpdaterDialog,
        )}
      >
        <div className="flex items-center justify-between px-2 py-2">
          <div className="flex gap-3">
            <span className="text-xl font-bold">{release?.version}</span>
            <span className="contents text-xs text-slate-500">
              {release?.date
                ? dayjs(release.date).format('YYYY-MM-DD HH:mm:ss')
                : m.common_invalid_date()}
            </span>
          </div>
          <Button
            variant="contained"
            size="small"
            onClick={() =>
              void openThat('https://github.com/MFSGA/Chimera/releases')
            }
          >
            {m.common_open()}
          </Button>
        </div>
        <div
          className={cn('h-[50vh] overflow-y-auto p-4', styles.MarkdownContent)}
        >
          {release && (
            <AppUpdateChangelog
              from={currentVersion}
              to={release.version}
              fallbackBody={release.body}
            />
          )}
          {update.error && (
            <p className="mt-3 text-sm text-red-600">{update.error}</p>
          )}
        </div>
        {pending && (
          <div className="mt-2 flex items-center gap-2">
            <LinearProgress
              className="flex-1"
              variant={update.total ? 'determinate' : 'indeterminate'}
              value={progress}
            />
            <span className="text-xs text-slate-500">
              {update.total ? `${progress.toFixed(2)}%` : update.phase}
            </span>
            {(update.phase === 'downloading' ||
              update.phase === 'verifying') && (
              <Button size="small" onClick={handleCancel}>
                {m.common_cancel()}
              </Button>
            )}
          </div>
        )}
      </div>
    </BaseDialog>
  );
}
