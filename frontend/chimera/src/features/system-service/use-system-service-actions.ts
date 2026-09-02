import {
  restartSidecar,
  useSystemService,
  type ServiceType,
} from '@chimera/interface';
import { useTransition } from 'react';
import * as m from '@/paraglide/messages';
import { formatError } from '@/utils';
import { message } from '@/utils/notification';

type ManualPromptOperation = Exclude<ServiceType, 'restart'>;
type ShowManualPrompt = (operation: ManualPromptOperation) => void;

const getInstallOperation = (status: string | undefined) => {
  if (status === 'not_installed') return 'install' as const;
  if (status === 'running' || status === 'stopped') return 'uninstall' as const;
  return null;
};

const getControlOperation = (status: string | undefined) => {
  if (status === 'running') return 'stop' as const;
  if (status === 'stopped') return 'start' as const;
  return null;
};

const notifyOperationFailure = (
  operation: ManualPromptOperation,
  error: unknown,
) => {
  const label =
    operation === 'install'
      ? m.settings_system_proxy_system_service_ctrl_failed_install()
      : operation === 'uninstall'
        ? m.settings_system_proxy_system_service_ctrl_failed_uninstall()
        : operation === 'stop'
          ? 'Stop failed'
          : 'Start failed';

  message(`${label}: ${formatError(error)}`, {
    kind: 'error',
    title: m.common_error(),
  });
};

export const useSystemServiceActions = (showManualPrompt: ShowManualPrompt) => {
  const { query, upsert } = useSystemService();
  const status = query.data?.status;
  const [installPending, startInstall] = useTransition();
  const [controlPending, startControl] = useTransition();

  const runOperation = async (operation: ManualPromptOperation) => {
    try {
      await upsert.mutateAsync(operation);
      await restartSidecar();
    } catch (error) {
      notifyOperationFailure(operation, error);
      showManualPrompt(operation);
    }
  };

  const handleInstall = () => {
    const operation = getInstallOperation(status);
    if (!operation) return;
    startInstall(() => runOperation(operation));
  };

  const handleControl = () => {
    const operation = getControlOperation(status);
    if (!operation) return;
    startControl(() => runOperation(operation));
  };

  return {
    query,
    status,
    isInstalled: status === 'running' || status === 'stopped',
    installPending,
    controlPending,
    isBusy: installPending || controlPending,
    handleInstall,
    handleControl,
  };
};
