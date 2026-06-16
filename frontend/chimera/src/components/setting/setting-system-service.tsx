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
import useSWR from 'swr';
import * as m from '@/paraglide/messages';
import { formatError } from '@/utils';
import { message } from '@/utils/notification';
import {
  ServerManualPromptDialogWrapper,
  useServerManualPromptDialog,
} from './modules/service-manual-prompt-dialog';

export const SettingSystemService = () => {
  const { query, upsert } = useSystemService();
  const coreStatusSWR = useSWR('/coreStatus', getCoreStatus, {
    refreshInterval: 2000,
    revalidateOnFocus: false,
  });

  const getInstallButtonString = () => {
    switch (query.data?.status) {
      case 'running':
      case 'stopped': {
        return m.settings_system_proxy_system_service_ctrl_uninstall();
      }

      case 'not_installed': {
        return m.settings_system_proxy_system_service_ctrl_install();
      }

      default:
        return undefined;
    }
  };
  const getControlButtonString = () => {
    switch (query.data?.status) {
      case 'running': {
        return m.settings_system_proxy_system_service_ctrl_stop();
      }

      case 'stopped': {
        return m.settings_system_proxy_system_service_ctrl_start();
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
            ? m.settings_system_proxy_system_service_ctrl_failed_install()
            : m.settings_system_proxy_system_service_ctrl_failed_uninstall()
        }: ${formatError(e)}`;

        message(errorMessage, {
          kind: 'error',
          title: 'Error',
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
          title: 'Error',
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
          title: 'Error',
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

  const getInstallStatusLabel = (status: string): string => {
    switch (status) {
      case 'not_installed':
        return 'Not Installed';
      case 'running':
        return 'Running';
      case 'stopped':
        return 'Stopped';
      default:
        return status;
    }
  };

  const currentCoreStatus = (() => {
    const status = coreStatusSWR.data?.[0];
    if (!status) return 'Unknown';
    if (
      isObject(status) &&
      Object.prototype.hasOwnProperty.call(status, 'Stopped')
    ) {
      const { Stopped } = status;
      return !!Stopped && Stopped.trim() ? `Stopped: ${Stopped}` : 'Stopped';
    }
    return 'Running';
  })();

  const currentRunType = coreStatusSWR.data?.[2]
    ? coreStatusSWR.data[2]
    : 'Unknown';

  const serviceCoreType = (() => {
    const type = serviceCoreInfos?.type;
    if (!type) return 'Unknown';
    return typeof type === 'string' ? type : type.clash;
  })();

  const currentCoreChangedAt = coreStatusSWR.data?.[1];
  const serviceCoreChangedAt = serviceCoreInfos?.state_changed_at;

  return (
    <BaseCard label={m.settings_system_proxy_system_service_ctrl_label()}>
      <ServerManualPromptDialogWrapper />
      <List disablePadding>
        <SwitchItem
          label={m.settings_system_proxy_service_mode_label()}
          disabled={isDisabled}
          checked={serviceMode.value || false}
          onChange={() => serviceMode.upsert(!serviceMode.value)}
        />

        {isDisabled && (
          <ListItem sx={{ pl: 0, pr: 0 }}>
            <Typography>
              {m.settings_system_proxy_service_mode_description()}
            </Typography>
          </ListItem>
        )}

        <ListItem sx={{ pl: 0, pr: 0 }}>
          <ListItemText
            primary={`Current Status: ${getInstallStatusLabel(query.data?.status ?? 'Unknown')}`}
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
                  Restart
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
              {'Refresh'}
            </Button>

            {import.meta.env.DEV && (
              <Button
                variant="contained"
                onClick={() => promptDialog.show('install')}
              >
                {'Prompt'}
              </Button>
            )}
          </div>
        </ListItem>

        <ListItem sx={{ pl: 0, pr: 0 }}>
          <ListItemText
            primary={'Service Name'}
            secondary={query.data?.name || 'Unknown'}
          />
        </ListItem>

        <ListItem sx={{ pl: 0, pr: 0 }}>
          <ListItemText
            primary={'Service Version'}
            secondary={query.data?.version || 'Unknown'}
          />
        </ListItem>

        <ListItem sx={{ pl: 0, pr: 0 }}>
          <ListItemText
            primary={'Server Version'}
            secondary={serviceServer?.version || 'Unknown'}
          />
        </ListItem>

        <ListItem sx={{ pl: 0, pr: 0 }}>
          <ListItemText
            primary={'App Core Status'}
            secondary={currentCoreStatus}
          />
        </ListItem>

        <ListItem sx={{ pl: 0, pr: 0 }}>
          <ListItemText primary={'Run Type'} secondary={currentRunType} />
        </ListItem>

        <ListItem sx={{ pl: 0, pr: 0 }}>
          <ListItemText primary={'Core Type'} secondary={serviceCoreType} />
        </ListItem>

        <ListItem sx={{ pl: 0, pr: 0 }}>
          <ListItemText
            primary={'Core Config Path'}
            secondary={serviceCoreInfos?.config_path || 'Unknown'}
          />
        </ListItem>

        <ListItem sx={{ pl: 0, pr: 0 }}>
          <ListItemText
            primary={'App Core State Changed At'}
            secondary={
              currentCoreChangedAt
                ? new Date(currentCoreChangedAt).toLocaleString()
                : 'Unknown'
            }
          />
        </ListItem>

        <ListItem sx={{ pl: 0, pr: 0 }}>
          <ListItemText
            primary={'Service Core State Changed At'}
            secondary={
              serviceCoreChangedAt
                ? new Date(serviceCoreChangedAt).toLocaleString()
                : 'Unknown'
            }
          />
        </ListItem>

        <ListItem sx={{ pl: 0, pr: 0 }}>
          <ListItemText
            primary={'Service Config Dir'}
            secondary={runtimeInfos?.service_config_dir || 'Unknown'}
          />
        </ListItem>

        <ListItem sx={{ pl: 0, pr: 0 }}>
          <ListItemText
            primary={'Service Data Dir'}
            secondary={runtimeInfos?.service_data_dir || 'Unknown'}
          />
        </ListItem>

        <ListItem sx={{ pl: 0, pr: 0 }}>
          <ListItemText
            primary={'Bound Config Dir'}
            secondary={runtimeInfos?.nyanpasu_config_dir || 'Unknown'}
          />
        </ListItem>

        <ListItem sx={{ pl: 0, pr: 0 }}>
          <ListItemText
            primary={'Bound Data Dir'}
            secondary={runtimeInfos?.nyanpasu_data_dir || 'Unknown'}
          />
        </ListItem>
      </List>
    </BaseCard>
  );
};

export default SettingSystemService;
