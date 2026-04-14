import {
  getCoreStatus,
  restartSidecar,
  useSetting,
  useSystemService,
} from '@chimera/interface';
import { BaseCard, SwitchItem } from '@chimera/ui';
import {
  Button,
  List,
  ListItem,
  ListItemText,
  Typography,
} from '@mui/material';
import { useMemoizedFn } from 'ahooks';
import { isObject } from 'lodash-es';
import { useTransition } from 'react';
import { useTranslation } from 'react-i18next';
import useSWR from 'swr';
import { formatError } from '@/utils';
import { message } from '@/utils/notification';
import {
  ServerManualPromptDialogWrapper,
  useServerManualPromptDialog,
} from './modules/service-manual-prompt-dialog';

export const SettingSystemService = () => {
  const { t } = useTranslation();

  const { query, upsert } = useSystemService();
  const coreStatusSWR = useSWR('/coreStatus', getCoreStatus, {
    refreshInterval: 2000,
    revalidateOnFocus: false,
  });

  const getInstallButtonString = () => {
    switch (query.data?.status) {
      case 'running':
      case 'stopped': {
        return t('uninstall');
      }

      case 'not_installed': {
        return t('install');
      }

      default:
        return undefined;
    }
  };
  const getControlButtonString = () => {
    switch (query.data?.status) {
      case 'running': {
        return t('stop');
      }

      case 'stopped': {
        return t('start');
      }

      default:
        return undefined;
    }
  };

  const isDisabled = query.data?.status === 'not_installed';

  const promptDialog = useServerManualPromptDialog();

  const [installOrUninstallPending, startInstallOrUninstall] = useTransition();
  const handleInstallClick = useMemoizedFn(() => {
    startInstallOrUninstall(async () => {
      try {
        switch (query.data?.status) {
          case 'running':
          case 'stopped':
            await upsert.mutateAsync('uninstall');
            break;

          case 'not_installed':
            await upsert.mutateAsync('install');
            break;

          default:
            break;
        }
        await restartSidecar();
      } catch (e) {
        const errorMessage = `${
          query.data?.status === 'not_installed'
            ? t('Failed to install')
            : t('Failed to uninstall')
        }: ${formatError(e)}`;

        message(errorMessage, {
          kind: 'error',
          title: t('Error'),
        });
        // If the installation fails, prompt the user to manually install the service
        promptDialog.show(
          query.data?.status === 'not_installed' ? 'install' : 'uninstall',
        );
      }
    });
  });

  const [serviceControlPending, startServiceControl] = useTransition();
  const handleControlClick = useMemoizedFn(() => {
    startServiceControl(async () => {
      try {
        switch (query.data?.status) {
          case 'running':
            await upsert.mutateAsync('stop');
            break;

          case 'stopped':
            await upsert.mutateAsync('start');
            break;

          default:
            break;
        }
        await restartSidecar();
      } catch (e) {
        const errorMessage =
          query.data?.status === 'running'
            ? `Stop failed: ${formatError(e)}`
            : `Start failed: ${formatError(e)}`;

        message(errorMessage, {
          kind: 'error',
          title: t('Error'),
        });
        // If start failed show a prompt to user to start the service manually
        promptDialog.show(query.data?.status === 'running' ? 'stop' : 'start');
      }
    });
  });

  const [serviceRestartPending, startServiceRestart] = useTransition();
  const handleRestartClick = useMemoizedFn(() => {
    startServiceRestart(async () => {
      try {
        await upsert.mutateAsync('restart');
        await restartSidecar();
      } catch (e) {
        message(`Restart failed: ${formatError(e)}`, {
          kind: 'error',
          title: t('Error'),
        });
      }
    });
  });

  const [refreshPending, startRefresh] = useTransition();
  const handleRefreshClick = useMemoizedFn(() => {
    startRefresh(async () => {
      await Promise.all([query.refetch(), coreStatusSWR.mutate()]);
    });
  });

  const serviceMode = useSetting('enable_service_mode');
  const serviceServer = query.data?.server;
  const isServiceInstalled = query.data?.status !== 'not_installed';
  const runtimeInfos = serviceServer?.runtime_infos;
  const serviceCoreInfos = serviceServer?.core_infos;

  const currentCoreStatus = (() => {
    const status = coreStatusSWR.data?.[0];
    if (!status) return t('Unknown');
    if (
      isObject(status) &&
      Object.prototype.hasOwnProperty.call(status, 'Stopped')
    ) {
      const { Stopped } = status;
      return !!Stopped && Stopped.trim()
        ? t('stopped_reason', { reason: Stopped })
        : t('stopped');
    }
    return t('running');
  })();

  const currentRunType = coreStatusSWR.data?.[2]
    ? t(coreStatusSWR.data[2])
    : t('Unknown');

  const serviceCoreType = (() => {
    const type = serviceCoreInfos?.type;
    if (!type) return t('Unknown');
    return typeof type === 'string' ? type : type.clash;
  })();

  const currentCoreChangedAt = coreStatusSWR.data?.[1];
  const serviceCoreChangedAt = serviceCoreInfos?.state_changed_at;

  return (
    <BaseCard label={t('System Service')}>
      <ServerManualPromptDialogWrapper />
      <List disablePadding>
        <SwitchItem
          label={t('Service Mode')}
          disabled={isDisabled}
          checked={serviceMode.value || false}
          onChange={() => serviceMode.upsert(!serviceMode.value)}
        />

        {isDisabled && (
          <ListItem sx={{ pl: 0, pr: 0 }}>
            <Typography>
              {t(
                'Information: To enable service mode, make sure the Clash Nyanpasu service is installed and started',
              )}
            </Typography>
          </ListItem>
        )}

        <ListItem sx={{ pl: 0, pr: 0 }}>
          <ListItemText
            primary={t('Current Status', {
              status: t(`${query.data?.status}`),
            })}
          />
          <div className="flex gap-2">
            {isServiceInstalled && (
              <Button
                variant="contained"
                onClick={handleControlClick}
                loading={serviceControlPending}
                disabled={
                  installOrUninstallPending ||
                  serviceControlPending ||
                  serviceRestartPending ||
                  refreshPending
                }
              >
                {getControlButtonString()}
              </Button>
            )}

            {/* {isServiceInstalled && (
                <Button
                  variant="contained"
                  onClick={handleRestartClick}
                  loading={serviceRestartPending}
                  disabled={
                    installOrUninstallPending ||
                    serviceControlPending ||
                    serviceRestartPending ||
                    refreshPending
                  }
                >
                  {t('Restart')}
                </Button>
              )} */}

            <Button
              variant="contained"
              onClick={handleInstallClick}
              loading={installOrUninstallPending}
              disabled={
                installOrUninstallPending ||
                serviceControlPending ||
                serviceRestartPending ||
                refreshPending
              }
            >
              {getInstallButtonString()}
            </Button>

            <Button
              variant="contained"
              onClick={handleRefreshClick}
              loading={refreshPending}
              disabled={
                installOrUninstallPending ||
                serviceControlPending ||
                serviceRestartPending ||
                refreshPending
              }
            >
              {t('Refresh')}
            </Button>

            {import.meta.env.DEV && (
              <Button
                variant="contained"
                onClick={() => promptDialog.show('install')}
              >
                {t('Prompt')}
              </Button>
            )}
          </div>
        </ListItem>

        <ListItem sx={{ pl: 0, pr: 0 }}>
          <ListItemText
            primary={t('Service Name')}
            secondary={query.data?.name || t('Unknown')}
          />
        </ListItem>

        <ListItem sx={{ pl: 0, pr: 0 }}>
          <ListItemText
            primary={t('Service Version')}
            secondary={query.data?.version || t('Unknown')}
          />
        </ListItem>

        <ListItem sx={{ pl: 0, pr: 0 }}>
          <ListItemText
            primary={t('Server Version')}
            secondary={serviceServer?.version || t('Unknown')}
          />
        </ListItem>

        <ListItem sx={{ pl: 0, pr: 0 }}>
          <ListItemText
            primary={t('App Core Status')}
            secondary={currentCoreStatus}
          />
        </ListItem>

        <ListItem sx={{ pl: 0, pr: 0 }}>
          <ListItemText primary={t('Run Type')} secondary={currentRunType} />
        </ListItem>

        <ListItem sx={{ pl: 0, pr: 0 }}>
          <ListItemText primary={t('Core Type')} secondary={serviceCoreType} />
        </ListItem>

        <ListItem sx={{ pl: 0, pr: 0 }}>
          <ListItemText
            primary={t('Core Config Path')}
            secondary={serviceCoreInfos?.config_path || t('Unknown')}
          />
        </ListItem>

        <ListItem sx={{ pl: 0, pr: 0 }}>
          <ListItemText
            primary={t('App Core State Changed At')}
            secondary={
              currentCoreChangedAt
                ? new Date(currentCoreChangedAt).toLocaleString()
                : t('Unknown')
            }
          />
        </ListItem>

        <ListItem sx={{ pl: 0, pr: 0 }}>
          <ListItemText
            primary={t('Service Core State Changed At')}
            secondary={
              serviceCoreChangedAt
                ? new Date(serviceCoreChangedAt).toLocaleString()
                : t('Unknown')
            }
          />
        </ListItem>

        <ListItem sx={{ pl: 0, pr: 0 }}>
          <ListItemText
            primary={t('Service Config Dir')}
            secondary={runtimeInfos?.service_config_dir || t('Unknown')}
          />
        </ListItem>

        <ListItem sx={{ pl: 0, pr: 0 }}>
          <ListItemText
            primary={t('Service Data Dir')}
            secondary={runtimeInfos?.service_data_dir || t('Unknown')}
          />
        </ListItem>

        <ListItem sx={{ pl: 0, pr: 0 }}>
          <ListItemText
            primary={t('Bound Config Dir')}
            secondary={runtimeInfos?.nyanpasu_config_dir || t('Unknown')}
          />
        </ListItem>

        <ListItem sx={{ pl: 0, pr: 0 }}>
          <ListItemText
            primary={t('Bound Data Dir')}
            secondary={runtimeInfos?.nyanpasu_data_dir || t('Unknown')}
          />
        </ListItem>
      </List>
    </BaseCard>
  );
};

export default SettingSystemService;
