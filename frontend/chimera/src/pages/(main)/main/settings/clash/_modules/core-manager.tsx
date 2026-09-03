import {
  inspectUpdater,
  useClashConnections,
  useClashCores,
  useSetting,
  type ClashCore_Serialize,
  type ClashCoresDetail,
  type UpdaterSummary,
} from '@chimera/interface';
import { cn } from '@chimera/utils';
import ArrowRightAltRounded from '~icons/material-symbols/arrow-right-alt-rounded';
import DeployedCodeUpdateOutlineRounded from '~icons/material-symbols/deployed-code-update-outline-rounded';
import RestartAltRounded from '~icons/material-symbols/restart-alt-rounded';
import { filesize } from 'filesize';
import { isObject } from 'lodash-es';
import { AnimatePresence, motion } from 'motion/react';
import { useMemo, useState } from 'react';
import { useBlockTask } from '@/components/providers/block-task-provider';
import {
  SettingsCard,
  SettingsCardContent,
  SettingsCardFooter,
} from '@/components/settings/settings-card';
import { Button } from '@/components/ui/button';
import { CircularProgress } from '@/components/ui/progress';
import TextMarquee from '@/components/ui/text-marquee';
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from '@/components/ui/tooltip';
import { OS } from '@/consts';
import useCoreIcon from '@/hooks/use-core-icon';
import { useLockFn } from '@/hooks/use-lock-fn';
import * as m from '@/paraglide/messages';
import { formatError } from '@/utils';
import { message } from '@/utils/notification';

function useCoreUpdateTask(
  core?: ClashCore_Serialize | null,
  item?: ClashCoresDetail | null,
) {
  const { query, updateCore } = useClashCores();
  const [updater, setUpdater] = useState<UpdaterSummary>();

  const task = useBlockTask(`core-manager-update-${core}`, async () => {
    try {
      const updaterId = await updateCore.mutateAsync(core!);

      if (!updaterId) {
        throw new Error('Failed to update');
      }

      await new Promise<void>((resolve, reject) => {
        const interval = setInterval(async () => {
          try {
            const result = await inspectUpdater(updaterId);
            setUpdater(result);

            if (
              isObject(result.downloader.state) &&
              Object.prototype.hasOwnProperty.call(
                result.downloader.state,
                'failed',
              )
            ) {
              clearInterval(interval);
              reject(result.downloader.state.failed);
              return;
            }

            if (result.state === 'done') {
              clearInterval(interval);
              resolve();
            }
          } catch (error) {
            clearInterval(interval);
            reject(error);
          }
        }, 100);
      });

      await query.refetch();

      message(m.settings_clash_core_update_success() + (item?.name ?? ''), {
        kind: 'info',
        title: m.common_success(),
      });
    } catch (error) {
      message(`${m.settings_clash_core_update_failed()}${formatError(error)}`, {
        kind: 'error',
        title: m.common_error(),
      });
    }
  });

  const progress = useMemo(() => {
    if (!updater || !task.isPending) {
      return 0;
    }

    const { downloaded, total } = updater.downloader;
    if (total <= 0) {
      return 0;
    }

    return Math.min((downloaded / total) * 100, 100);
  }, [updater, task.isPending]);

  const stateLabel = useMemo(() => {
    if (!updater || !task.isPending) {
      return null;
    }

    const state = updater.state;

    if (state === 'downloading') {
      const { downloaded, total, speed } = updater.downloader;
      return `${filesize(downloaded, { standard: 'iec' })} / ${filesize(total, { standard: 'iec' })} · ${filesize(speed ?? 0, { standard: 'iec' })}/s`;
    }

    if (state === 'decompressing') {
      return m.settings_clash_core_manager_card_decompressing();
    }

    if (state === 'replacing') {
      return m.settings_clash_core_manager_card_replacing();
    }

    if (state === 'restarting') {
      return m.settings_clash_core_manager_card_restarting();
    }

    if (state === 'done') {
      return m.settings_clash_core_manager_card_done();
    }

    return null;
  }, [updater, task.isPending]);

  return { task, progress, stateLabel };
}

function UpdateProgressBar({
  isPending,
  progress,
}: {
  isPending: boolean;
  progress: number;
}) {
  if (!isPending) {
    return null;
  }

  return (
    <motion.div
      className="bg-primary/10 absolute inset-0 origin-left"
      data-slot="core-manager-update-progress"
      initial={{ scaleX: 0 }}
      animate={{ scaleX: progress / 100 }}
      transition={{ duration: 0.3, ease: 'easeOut' }}
    />
  );
}

function CoreItem({
  core,
  item,
  onClick,
}: {
  core: ClashCore_Serialize;
  item: ClashCoresDetail;
  onClick: (core: ClashCore_Serialize) => void;
}) {
  const { value: currentCore } = useSetting('clash_core');
  const icon = useCoreIcon(core);
  const isSelected = core === currentCore;
  const haveNewVersion = item.latestVersion
    ? item.latestVersion !== item.currentVersion
    : false;
  const {
    task: updateCoreTask,
    progress,
    stateLabel: updaterStateLabel,
  } = useCoreUpdateTask(core, item);

  return (
    <Button
      variant={isSelected ? 'raised' : 'basic'}
      data-selected={isSelected}
      data-downloading={updateCoreTask.isPending}
      data-slot="core-manager-item"
      className={cn(
        'relative h-auto w-full min-w-0 overflow-hidden rounded-2xl p-2 text-left',
        'flex items-center gap-2',
      )}
      onClick={() => {
        if (!updateCoreTask.isPending) {
          onClick(core);
        }
      }}
    >
      <UpdateProgressBar
        isPending={updateCoreTask.isPending}
        progress={progress}
      />

      <div className="relative size-12 shrink-0">
        <img className="size-full" src={icon} alt={item.name} />
      </div>

      <div className="relative flex min-w-0 flex-1 flex-col gap-1">
        <TextMarquee>{item.name}</TextMarquee>

        <TextMarquee className="text-sm">
          {updateCoreTask.isPending && updaterStateLabel ? (
            <span className="text-emerald-700">{updaterStateLabel}</span>
          ) : haveNewVersion ? (
            <span className="flex items-center gap-1">
              <span>{item.currentVersion}</span>
              <ArrowRightAltRounded />
              <span className="text-emerald-700">{item.latestVersion}</span>
            </span>
          ) : (
            item.currentVersion
          )}
        </TextMarquee>
      </div>

      {haveNewVersion && (
        <div className="m-2">
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                className="size-8"
                variant="stroked"
                icon
                onClick={(event) => {
                  event.preventDefault();
                  event.stopPropagation();
                  void updateCoreTask.execute();
                }}
                loading={updateCoreTask.isPending}
                asChild
              >
                <span>
                  <DeployedCodeUpdateOutlineRounded />
                </span>
              </Button>
            </TooltipTrigger>

            <TooltipContent>
              {m.settings_clash_core_manager_card_click_to_update()}
            </TooltipContent>
          </Tooltip>
        </div>
      )}
    </Button>
  );
}

export default function CoreManager() {
  const {
    query: clashCores,
    upsert: switchCore,
    restartSidecar,
    fetchRemote,
  } = useClashCores();
  const { deleteConnections } = useClashConnections();
  const { value: currentCoreKey } = useSetting('clash_core');
  const currentCoreIcon = useCoreIcon(currentCoreKey);
  const currentCore = currentCoreKey && clashCores.data?.[currentCoreKey];

  const switchCoreTask = useBlockTask(
    'core-manager-switch',
    async (core: ClashCore_Serialize) => {
      try {
        try {
          await deleteConnections.mutateAsync(undefined);
        } catch (error) {
          console.error(error);
        }

        await switchCore.mutateAsync(core);

        message(m.settings_clash_core_manager_card_loading_success(), {
          kind: 'info',
          title: m.common_success(),
        });
      } catch (error) {
        message(
          `${m.settings_clash_core_manager_card_loading_error()}\n${formatError(error)}`,
          {
            kind: 'error',
            title: m.common_error(),
          },
        );
      }
    },
  );

  const restartSidecarTask = useBlockTask(
    'core-manager-restart-sidecar',
    async () => {
      try {
        await restartSidecar();
        message(m.settings_clash_core_manager_card_restart_sidecar_success(), {
          kind: 'info',
          title: m.common_success(),
        });
      } catch (error) {
        message(
          `${m.settings_clash_core_manager_card_restart_sidecar_error()}\n${formatError(error)}`,
          {
            kind: 'error',
            title: m.common_error(),
          },
        );
      }
    },
  );

  const handleFetchRemote = useLockFn(async () => {
    try {
      await fetchRemote.mutateAsync();
    } catch (error) {
      message(
        `${m.settings_clash_core_fetch_failed()}\n${formatError(error)}`,
        {
          kind: 'error',
          title: m.common_error(),
        },
      );
    }
  });

  const isLoading =
    clashCores.isPending ||
    switchCoreTask.isPending ||
    restartSidecarTask.isPending;
  const haveNewVersion = currentCore?.latestVersion
    ? currentCore.latestVersion !== currentCore.currentVersion
    : false;
  const currentCoreUpdate = useCoreUpdateTask(currentCoreKey, currentCore);

  return (
    <SettingsCard data-slot="core-manager-card" className="relative">
      <AnimatePresence initial={false}>
        {isLoading && (
          <motion.div
            data-slot="core-manager-card-mask"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            className={cn(
              'bg-primary/10 absolute inset-0 z-50 backdrop-blur-3xl',
              'flex flex-col items-center justify-center gap-4',
            )}
          >
            <CircularProgress className="size-12" indeterminate />
            <p>{m.settings_clash_core_manager_card_loading()}</p>
          </motion.div>
        )}
      </AnimatePresence>

      <SettingsCardContent data-slot="core-manager-card-content">
        <div
          className={cn(
            'bg-surface-variant relative flex items-center gap-3 overflow-hidden rounded-2xl p-4',
          )}
          data-slot="core-manager-current"
        >
          <UpdateProgressBar
            isPending={currentCoreUpdate.task.isPending}
            progress={currentCoreUpdate.progress}
          />

          <div className="relative size-12 shrink-0">
            <img
              src={currentCoreIcon}
              alt={currentCore?.name ?? ''}
              className="size-full"
            />
          </div>

          <div className="relative min-w-0 flex-1">
            <p className="truncate font-medium">{currentCore?.name ?? '-'}</p>

            <p className="flex items-center gap-1 text-sm">
              {currentCoreUpdate.task.isPending &&
              currentCoreUpdate.stateLabel ? (
                <span className="text-emerald-700">
                  {currentCoreUpdate.stateLabel}
                </span>
              ) : haveNewVersion ? (
                <>
                  <span>{currentCore?.currentVersion}</span>
                  <ArrowRightAltRounded />
                  <span className="text-emerald-700">
                    {currentCore?.latestVersion}
                  </span>
                </>
              ) : (
                (currentCore?.currentVersion ?? '-')
              )}
            </p>
          </div>

          <div className="relative mr-2 flex items-center gap-3">
            {haveNewVersion && (
              <Tooltip>
                <TooltipTrigger asChild>
                  <Button
                    variant="stroked"
                    icon
                    onClick={() => void currentCoreUpdate.task.execute()}
                    loading={currentCoreUpdate.task.isPending}
                  >
                    <DeployedCodeUpdateOutlineRounded className="size-5" />
                  </Button>
                </TooltipTrigger>

                <TooltipContent>
                  {m.settings_clash_core_manager_card_click_to_update()}
                </TooltipContent>
              </Tooltip>
            )}

            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  icon
                  variant="stroked"
                  onClick={() => void restartSidecarTask.execute()}
                >
                  <RestartAltRounded className="size-5" />
                </Button>
              </TooltipTrigger>

              <TooltipContent>
                {m.settings_clash_core_manager_card_restart_sidecar()}
              </TooltipContent>
            </Tooltip>
          </div>
        </div>

        <div className="grid grid-cols-1 gap-2 md:grid-cols-2">
          {Object.entries(clashCores.data ?? {}).map(([core, item]) => {
            if (core === currentCoreKey) {
              return null;
            }

            return (
              <CoreItem
                key={item.name}
                core={core as ClashCore_Serialize}
                item={item}
                onClick={(nextCore) => void switchCoreTask.execute(nextCore)}
              />
            );
          })}
        </div>
      </SettingsCardContent>

      {OS !== 'linux' && (
        <SettingsCardFooter className="gap-2">
          <Button
            variant="flat"
            onClick={handleFetchRemote}
            loading={fetchRemote.isPending}
          >
            {m.settings_clash_core_manager_card_fetch_remote()}
          </Button>
        </SettingsCardFooter>
      )}
    </SettingsCard>
  );
}
