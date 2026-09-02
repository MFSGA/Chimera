import { useCoreStatus } from '@chimera/interface';
import { BaseCard, SwitchItem } from '@chimera/ui';
import {
  Button,
  List,
  ListItem,
  ListItemText,
  Typography,
} from '@mui/material';
import { useMemoizedFn } from 'ahooks';
import { useTransition } from 'react';
import {
  getAppCoreStatusLabel,
  getServiceCoreTypeLabel,
  getSystemServiceStatusLabel,
} from '@/features/system-service/system-service-display';
import { useSystemServiceActions } from '@/features/system-service/use-system-service-actions';
import { useSystemServiceMode } from '@/features/system-service/use-system-service-mode';
import * as m from '@/paraglide/messages';
import {
  ServerManualPromptDialogWrapper,
  useServerManualPromptDialog,
} from './modules/service-manual-prompt-dialog';

export const SettingSystemService = () => {
  const coreStatusQuery = useCoreStatus();
  const promptDialog = useServerManualPromptDialog();
  const {
    query,
    isInstalled: isServiceInstalled,
    installPending: installOrUninstallPending,
    controlPending: serviceControlPending,
    isBusy: serviceActionPending,
    handleInstall: handleInstallClick,
    handleControl: handleControlClick,
  } = useSystemServiceActions(promptDialog.show);
  const serviceMode = useSystemServiceMode();

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

  const isDisabled = serviceMode.isNotInstalled;

  const [refreshPending, startRefresh] = useTransition();
  const handleRefreshClick = useMemoizedFn(() => {
    startRefresh(async () => {
      await Promise.all([query.refetch(), coreStatusQuery.refetch()]);
    });
  });

  const serviceServer = query.data?.server;
  const runtimeInfos = serviceServer?.runtime_infos;
  const serviceCoreInfos = serviceServer?.core_infos;

  const currentCoreStatus = getAppCoreStatusLabel(coreStatusQuery.data?.status);
  const currentRunType = coreStatusQuery.data?.type ?? m.common_unknown();
  const serviceCoreType = getServiceCoreTypeLabel(serviceCoreInfos?.type);

  const currentCoreChangedAt = coreStatusQuery.data?.startAt;
  const serviceCoreChangedAt = serviceCoreInfos?.state_changed_at;

  return (
    <BaseCard label={m.settings_system_proxy_system_service_ctrl_label()}>
      <ServerManualPromptDialogWrapper />
      <List disablePadding>
        <SwitchItem
          label={m.settings_system_proxy_service_mode_label()}
          disabled={isDisabled}
          checked={serviceMode.value}
          onChange={serviceMode.toggle}
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
              getSystemServiceStatusLabel(query.data?.status)
            }
          />
          <div className="flex gap-2">
            {isServiceInstalled && (
              <Button
                variant="contained"
                onClick={handleControlClick}
                loading={serviceControlPending}
                disabled={serviceActionPending || refreshPending}
              >
                {getControlButtonString()}
              </Button>
            )}

            <Button
              variant="contained"
              onClick={handleInstallClick}
              loading={installOrUninstallPending}
              disabled={serviceActionPending || refreshPending}
            >
              {getInstallButtonString()}
            </Button>

            <Button
              variant="contained"
              onClick={handleRefreshClick}
              loading={refreshPending}
              disabled={serviceActionPending || refreshPending}
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
