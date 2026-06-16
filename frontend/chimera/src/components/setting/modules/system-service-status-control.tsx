import { restartSidecar, useSystemService } from '@chimera/interface';
import { LoadingButton } from '@chimera/ui';
import { Button, ListItem, ListItemText } from '@mui/material';
import { useMemoizedFn } from 'ahooks';
import { useTransition } from 'react';
import * as m from '@/paraglide/messages';
import { formatError } from '@/utils';
import { message } from '@/utils/notification';
import { useServerManualPromptDialog } from './service-manual-prompt-dialog';

const getInstallButtonLabel = (
  status: 'running' | 'stopped' | 'not_installed' | undefined,
) => {
  switch (status) {
    case 'running':
    case 'stopped':
      return m.settings_system_proxy_system_service_ctrl_uninstall();
    case 'not_installed':
      return m.settings_system_proxy_system_service_ctrl_install();
    default:
      return m.settings_system_proxy_system_service_ctrl_install();
  }
};

const getControlButtonLabel = (
  status: 'running' | 'stopped' | 'not_installed' | undefined,
) => {
  switch (status) {
    case 'running':
      return m.settings_system_proxy_system_service_ctrl_stop();
    case 'stopped':
      return m.settings_system_proxy_system_service_ctrl_start();
    default:
      return m.settings_system_proxy_system_service_ctrl_start();
  }
};

export const ServiceStatusControl = () => {
  const { query, upsert } = useSystemService();
  const status = query.data?.status;
  // todo use enum
  const isDisabled = status === 'not_installed';
  const promptDialog = useServerManualPromptDialog();

  const [installOrUninstallPending, startInstallOrUninstall] = useTransition();
  const [serviceControlPending, startServiceControl] = useTransition();
  const isPending = installOrUninstallPending || serviceControlPending;

  const handleInstallClick = useMemoizedFn(() => {
    startInstallOrUninstall(async () => {
      try {
        switch (status) {
          case 'running':
          case 'stopped':
            await upsert.mutateAsync('uninstall');
            break;
          case 'not_installed':
            await upsert.mutateAsync('install');
            break;
          default:
            return;
        }
        await restartSidecar();
      } catch (error) {
        const errorMessage = `${
          status === 'not_installed'
            ? m.settings_system_proxy_system_service_ctrl_failed_install()
            : m.settings_system_proxy_system_service_ctrl_failed_uninstall()
        }: ${formatError(error)}`;
        message(errorMessage, { kind: 'error', title: 'Error' });

        if (status === 'not_installed') {
          promptDialog.show('install');
        } else if (status) {
          promptDialog.show('uninstall');
        }
      }
    });
  });

  const handleControlClick = useMemoizedFn(() => {
    startServiceControl(async () => {
      try {
        switch (status) {
          case 'running':
            await upsert.mutateAsync('stop');
            break;
          case 'stopped':
            await upsert.mutateAsync('start');
            break;
          default:
            return;
        }
        await restartSidecar();
      } catch (error) {
        const errorMessage =
          status === 'running'
            ? `Stop failed: ${formatError(error)}`
            : `Start failed: ${formatError(error)}`;
        message(errorMessage, { kind: 'error', title: 'Error' });

        if (status === 'running') {
          promptDialog.show('stop');
        } else if (status) {
          promptDialog.show('start');
        }
      }
    });
  });

  return (
    <ListItem sx={{ pl: 0, pr: 0 }}>
      <ListItemText primary={`Current Status: ${status ?? 'Unknown'}`} />

      <div className="flex gap-2">
        {!isDisabled && (
          <LoadingButton
            variant="contained"
            onClick={handleControlClick}
            loading={serviceControlPending}
            disabled={isPending}
          >
            {getControlButtonLabel(status)}
          </LoadingButton>
        )}

        <LoadingButton
          variant="contained"
          onClick={handleInstallClick}
          loading={installOrUninstallPending}
          disabled={isPending}
        >
          {getInstallButtonLabel(status)}
        </LoadingButton>

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
  );
};
