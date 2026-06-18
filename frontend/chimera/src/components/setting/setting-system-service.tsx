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
          title: m.common_error(),
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
          title: m.common_error(),
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
          title: m.common_error(),
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
        return m.dashboard_widget_core_service_not_installed();
      case 'running':
        return m.dashboard_widget_core_status_running();
      case 'stopped':
        return m.dashboard_widget_core_status_stopped();
      default:
        return m.common_unknown();
    }
  };

  const currentCoreStatus = (() => {
    const status = coreStatusSWR.data?.[0];
    if (!status) return m.common_unknown();
    if (
      isObject(status) &&
      Object.prototype.hasOwnProperty.call(status, 'Stopped')
    ) {
      const { Stopped } = status;
      return !!Stopped && Stopped.trim()
        ? m.dashboard_widget_core_stopped_with_message({ message: Stopped })
        : m.dashboard_widget_core_status_stopped();
    }
    return m.dashboard_widget_core_status_running();
  })();

  const currentRunType = coreStatusSWR.data?.[2]
    ? coreStatusSWR.data[2]
    : m.common_unknown();

  const serviceCoreType = (() => {
    const type = serviceCoreInfos?.type;
    if (!type) return m.common_unknown();
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
            primary={
              m.common_current_status() +
              ': ' +
              getInstallStatusLabel(query.data?.status ?? m.common_unknown())
            }
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
              {m.common_refresh()}
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
            primary={m.settings_service_name_label()}
            secondary={query.data?.name || m.common_unknown()}
          />
        </ListItem>

        <ListItem sx={{ pl: 0, pr: 0 }}>
          <ListItemText
            primary={m.settings_service_version_label()}
            secondary={query.data?.version || m.common_unknown()}
          />
        </ListItem>

        <ListItem sx={{ pl: 0, pr: 0 }}>
          <ListItemText
            primary={m.settings_server_version_label()}
            secondary={serviceServer?.version || m.common_unknown()}
          />
        </ListItem>

        <ListItem sx={{ pl: 0, pr: 0 }}>
          <ListItemText
            primary={m.settings_app_core_status_label()}
            secondary={currentCoreStatus}
          />
        </ListItem>

        <ListItem sx={{ pl: 0, pr: 0 }}>
          <ListItemText
            primary={m.settings_run_type_label()}
            secondary={currentRunType}
          />
        </ListItem>

        <ListItem sx={{ pl: 0, pr: 0 }}>
          <ListItemText
            primary={m.settings_core_type_label()}
            secondary={serviceCoreType}
          />
        </ListItem>

        <ListItem sx={{ pl: 0, pr: 0 }}>
          <ListItemText
            primary={m.settings_core_config_path_label()}
            secondary={serviceCoreInfos?.config_path || m.common_unknown()}
          />
        </ListItem>

        <ListItem sx={{ pl: 0, pr: 0 }}>
          <ListItemText
            primary={m.settings_app_core_state_changed_at_label()}
            secondary={
              currentCoreChangedAt
                ? new Date(currentCoreChangedAt).toLocaleString()
                : m.common_unknown()
            }
          />
        </ListItem>

        <ListItem sx={{ pl: 0, pr: 0 }}>
          <ListItemText
            primary={m.settings_service_core_state_changed_at_label()}
            secondary={
              serviceCoreChangedAt
                ? new Date(serviceCoreChangedAt).toLocaleString()
                : m.common_unknown()
            }
          />
        </ListItem>

        <ListItem sx={{ pl: 0, pr: 0 }}>
          <ListItemText
            primary={m.settings_service_config_dir_label()}
            secondary={runtimeInfos?.service_config_dir || m.common_unknown()}
          />
        </ListItem>

        <ListItem sx={{ pl: 0, pr: 0 }}>
          <ListItemText
            primary={m.settings_service_data_dir_label()}
            secondary={runtimeInfos?.service_data_dir || m.common_unknown()}
          />
        </ListItem>

        <ListItem sx={{ pl: 0, pr: 0 }}>
          <ListItemText
            primary={m.settings_bound_config_dir_label()}
            secondary={runtimeInfos?.nyanpasu_config_dir || m.common_unknown()}
          />
        </ListItem>

        <ListItem sx={{ pl: 0, pr: 0 }}>
          <ListItemText
            primary={m.settings_bound_data_dir_label()}
            secondary={runtimeInfos?.nyanpasu_data_dir || m.common_unknown()}
          />
        </ListItem>
      </List>
    </BaseCard>
  );
};

export default SettingSystemService;
