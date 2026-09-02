import { LoadingButton } from '@chimera/ui';
import { Button, ListItem, ListItemText } from '@mui/material';
import { useSystemServiceActions } from '@/features/system-service/use-system-service-actions';
import * as m from '@/paraglide/messages';
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
  const promptDialog = useServerManualPromptDialog();
  const {
    status,
    installPending,
    controlPending,
    isBusy,
    handleInstall,
    handleControl,
  } = useSystemServiceActions(promptDialog.show);
  // todo use enum
  const isDisabled = status === 'not_installed';

  return (
    <ListItem sx={{ pl: 0, pr: 0 }}>
      <ListItemText
        primary={
          m.common_current_status() + ': ' + (status ?? m.common_unknown())
        }
      />

      <div className="flex gap-2">
        {!isDisabled && (
          <LoadingButton
            variant="contained"
            onClick={handleControl}
            loading={controlPending}
            disabled={isBusy}
          >
            {getControlButtonLabel(status)}
          </LoadingButton>
        )}

        <LoadingButton
          variant="contained"
          onClick={handleInstall}
          loading={installPending}
          disabled={isBusy}
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
