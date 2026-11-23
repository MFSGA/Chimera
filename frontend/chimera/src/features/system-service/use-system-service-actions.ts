import {
  CLASH_CORE_STATUS_QUERY_KEY,
  CORE_ACTOR_STATUS_QUERY_KEY,
  restartSidecar,
  useSystemService,
} from '@chimera/interface';
import { useQueryClient } from '@tanstack/react-query';
import { useTransition } from 'react';
import * as m from '@/paraglide/messages';
import { formatError } from '@/utils';
import { message } from '@/utils/notification';
import {
  runServiceOperation,
  type ManualPromptOperation,
} from './service-operation';

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
  const queryClient = useQueryClient();
  const status = query.data?.status;
  const [installPending, startInstall] = useTransition();
  const [controlPending, startControl] = useTransition();

  const runOperation = async (operation: ManualPromptOperation) => {
    const result = await runServiceOperation(
      operation,
      (nextOperation) => upsert.mutateAsync(nextOperation),
      restartSidecar,
    );

    // A successful OS Service command can still be followed by an uncertain
    // CoreActor result. Refresh both projections without inferring success.
    void queryClient.invalidateQueries({
      queryKey: [CORE_ACTOR_STATUS_QUERY_KEY],
    });
    void queryClient.invalidateQueries({
      queryKey: [CLASH_CORE_STATUS_QUERY_KEY],
    });

    if (result.stage === 'service_failed') {
      notifyOperationFailure(operation, result.error);
      showManualPrompt(operation);
    } else if (result.stage === 'core_restart_failed') {
      message(
        `${m.settings_clash_core_restart_failed()}: ${formatError(result.error)}`,
        { kind: 'error', title: m.common_error() },
      );
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
