import { commands, useSetting } from '@chimera/interface';
import { Button } from '@chimera/ui/button';
import { Card, CardContent, CardFooter, CardHeader } from '@chimera/ui/card';
import {
  Modal,
  ModalClose,
  ModalContent,
  ModalTitle,
  ModalTrigger,
} from '@chimera/ui/modal';
import { LinearProgress } from '@chimera/ui/progress';
import { ScrollArea } from '@chimera/ui/scroll-area';
import { SwitchItem } from '@chimera/ui/switch';
import { PropsWithChildren, useEffect, useState } from 'react';
import AnimatedLogo from '@/components/logo/animated-logo';
import { useChimeraUpdate } from '@/components/providers/chimera-update-provider';
import ChimeraUpdatePreferences from '@/components/setting/chimera-update-preferences';
import AppUpdateChangelog from '@/components/updater/app-update-changelog';
import { useLockFn } from '@/hooks/use-lock-fn';
import {
  Action as AboutAction,
  Route as AboutRoute,
} from '@/pages/(main)/main/settings/about/route';
import * as m from '@/paraglide/messages';
import { formatError } from '@/utils';
import { message } from '@/utils/notification';
import {
  SettingsCard,
  SettingsCardContent,
  SettingsCardFooter,
} from '../../_modules/settings-card';

const TITLE = 'Clash Chimera~(∠・ω< )⌒☆';
const GITHUB_RELEASES_URL = 'https://github.com/MFSGA/Chimera/releases';

const AutoCheckUpdate = () => {
  const { value, upsert, isPending } = useSetting('enable_auto_check_update');

  return (
    <SwitchItem
      data-slot="auto-check-updates-switch"
      className="rounded-[20px]"
      checked={value ?? true}
      onCheckedChange={(checked) => upsert(checked)}
      loading={isPending}
    >
      <p className="truncate">{m.settings_label_about_auto_check_updates()}</p>
    </SwitchItem>
  );
};

const NewVersionModal = ({
  children,
  currentVersion,
}: PropsWithChildren<{ currentVersion: string }>) => {
  const { action } = AboutRoute.useSearch();
  const { snapshot, downloadUpdate, cancelDownload, installUpdate } =
    useChimeraUpdate();
  const release = snapshot?.release;
  const cancelling = snapshot?.phase === 'cancelling';
  const downloading =
    snapshot?.phase === 'downloading' ||
    snapshot?.phase === 'verifying' ||
    cancelling;
  const installing = snapshot?.phase === 'installing';
  const progress =
    snapshot?.downloaded && snapshot.total
      ? (snapshot.downloaded / snapshot.total) * 100
      : 0;
  const [open, setOpen] = useState(false);

  useEffect(() => {
    if (action === AboutAction.NEED_UPDATE) setOpen(true);
  }, [action]);

  const handleUpdate = useLockFn(async () => {
    if (!release) return;
    try {
      if (snapshot?.phase === 'ready') await installUpdate();
      else await downloadUpdate();
    } catch (error) {
      message(formatError(error), { kind: 'error', title: m.common_error() });
    }
  });

  return (
    <Modal open={open} onOpenChange={(next) => !installing && setOpen(next)}>
      <ModalTrigger asChild>{children}</ModalTrigger>
      <ModalContent>
        <Card className="max-w-3xl min-w-96">
          <CardHeader>
            <ModalTitle>
              {m.settings_label_about_update_has_new_version()}
            </ModalTitle>
          </CardHeader>
          <CardContent asChild>
            <ScrollArea className="max-h-[80dvh]">
              {downloading || installing ? (
                <div className="flex flex-col gap-2">
                  <div className="flex items-center gap-2">
                    {installing
                      ? m.settings_label_about_update_installing()
                      : cancelling
                        ? m.settings_about_update_status_cancelling()
                        : snapshot?.phase === 'verifying'
                          ? m.settings_about_update_status_verifying()
                          : m.settings_about_update_status_downloading()}
                    <span className="text-xs text-slate-500">
                      {snapshot?.total ? `${progress.toFixed(2)}%` : ''}
                    </span>
                  </div>
                  <LinearProgress
                    className="w-full"
                    value={progress}
                    indeterminate={!snapshot?.total}
                  />
                </div>
              ) : release ? (
                <AppUpdateChangelog
                  from={currentVersion}
                  to={release.version}
                  fallbackBody={release.body}
                />
              ) : null}
              {snapshot?.error && (
                <p className="mt-3 text-sm text-red-600">{snapshot.error}</p>
              )}
            </ScrollArea>
          </CardContent>
          <CardFooter className="gap-2">
            <Button
              variant="flat"
              loading={downloading || installing}
              disabled={downloading || installing}
              onClick={handleUpdate}
            >
              {snapshot?.phase === 'ready'
                ? m.settings_about_update_install_action()
                : m.settings_label_about_update_to_update_button()}
            </Button>
            {downloading && !cancelling && (
              <Button
                variant="flat"
                onClick={() =>
                  void cancelDownload().catch((error) =>
                    message(formatError(error), {
                      kind: 'error',
                      title: m.common_error(),
                    }),
                  )
                }
              >
                {m.common_cancel()}
              </Button>
            )}
            {!installing && <ModalClose>{m.common_close()}</ModalClose>}
          </CardFooter>
        </Card>
      </ModalContent>
    </Modal>
  );
};

export default function ChimeraVersion() {
  const {
    currentVersion,
    hasNewVersion,
    isChecking,
    checkNewVersion,
    isSupported,
  } = useChimeraUpdate();

  const handleUpdateToGithubReleases = useLockFn(
    async () => await commands.openThat(GITHUB_RELEASES_URL),
  );

  const handleCheckNewVersion = useLockFn(async () => {
    try {
      const update = await checkNewVersion();
      if (update.release) {
        message(m.settings_label_about_update_has_new_version(), {
          kind: 'info',
          title: m.settings_label_about_update(),
        });
      } else if (update.phase === 'up_to_date') {
        message(m.settings_label_about_update_no_update(), {
          kind: 'info',
          title: m.settings_label_about_update(),
        });
      }
    } catch (error) {
      message(formatError(error), {
        kind: 'error',
        title: m.settings_about_update_check_failed(),
      });
    }
  });

  return (
    <SettingsCard className="space-y-2">
      <SettingsCardContent className="items-center gap-4">
        <div className="p-4">
          <AnimatedLogo className="size-32" indeterminate />
        </div>
        <div className="truncate text-base font-bold">{TITLE}</div>
        <div className="text-sm font-semibold">
          {m.settings_label_about_version({ version: currentVersion })}
        </div>
      </SettingsCardContent>
      {isSupported ? (
        <SettingsCardFooter className="flex-col gap-2">
          <AutoCheckUpdate />
          <ChimeraUpdatePreferences />
          {hasNewVersion ? (
            <NewVersionModal currentVersion={currentVersion}>
              <Button variant="flat" className="w-full">
                {m.settings_label_about_update_has_new_version()}
              </Button>
            </NewVersionModal>
          ) : (
            <Button
              variant="flat"
              className="w-full"
              onClick={handleCheckNewVersion}
              loading={isChecking}
            >
              {m.settings_label_about_update()}
            </Button>
          )}
        </SettingsCardFooter>
      ) : (
        <SettingsCardFooter>
          <Button
            variant="flat"
            className="w-full"
            onClick={handleUpdateToGithubReleases}
          >
            {m.settings_label_about_update_to_github_releases()}
          </Button>
        </SettingsCardFooter>
      )}
    </SettingsCard>
  );
}
