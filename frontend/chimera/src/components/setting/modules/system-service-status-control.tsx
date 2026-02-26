import { restartSidecar, useSystemService } from '@chimera/interface';
import { LoadingButton } from '@chimera/ui';
import { Button, ListItem, ListItemText } from '@mui/material';
import { useMemoizedFn } from 'ahooks';
import { useTransition } from 'react';
import { useTranslation } from 'react-i18next';
import { formatError } from '@/utils';
import { message } from '@/utils/notification';
import { useServerManualPromptDialog } from './service-manual-prompt-dialog';

const getInstallButtonLabel = (
  status: 'running' | 'stopped' | 'not_installed' | undefined,
  t: (key: string) => string,
) => {
  switch (status) {
    case 'running':
    case 'stopped':
      return t('uninstall');
    case 'not_installed':
      return t('install');
    default:
      return t('install');
  }
};

const getControlButtonLabel = (
  status: 'running' | 'stopped' | 'not_installed' | undefined,
  t: (key: string) => string,
) => {
  switch (status) {
    case 'running':
      return t('stop');
    case 'stopped':
      return t('start');
    default:
      return t('start');
  }
};

export const ServiceStatusControl = () => {
  const { t } = useTranslation();
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
            ? t('Failed to install')
            : t('Failed to uninstall')
        }: ${formatError(error)}`;
        message(errorMessage, { kind: 'error', title: t('Error') });

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
        message(errorMessage, { kind: 'error', title: t('Error') });

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
      <ListItemText
        primary={t('Current Status', {
          status: t(`${status}`),
        })}
      />

      <div className="flex gap-2">
        {!isDisabled && (
          <LoadingButton
            variant="contained"
            onClick={handleControlClick}
            loading={serviceControlPending}
            disabled={isPending}
          >
            {getControlButtonLabel(status, t)}
          </LoadingButton>
        )}

        <LoadingButton
          variant="contained"
          onClick={handleInstallClick}
          loading={installOrUninstallPending}
          disabled={isPending}
        >
          {getInstallButtonLabel(status, t)}
        </LoadingButton>

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
  );
};
