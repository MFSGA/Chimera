import { useCoreStatus } from '@chimera/interface';
import { useTransition } from 'react';
import {
  ServerManualPromptDialogWrapper,
  useServerManualPromptDialog,
} from '@/components/setting/modules/service-manual-prompt-dialog';
import { Button } from '@/components/ui/button';
import {
  Card,
  CardContent,
  CardFooter,
  CardHeader,
} from '@/components/ui/card';
import {
  Modal,
  ModalClose,
  ModalContent,
  ModalTitle,
  ModalTrigger,
} from '@/components/ui/modal';
import {
  getAppCoreStatusLabel,
  getServiceCoreTypeLabel,
  getSystemServiceStatusLabel,
} from '@/features/system-service/system-service-display';
import { useSystemServiceActions } from '@/features/system-service/use-system-service-actions';
import * as m from '@/paraglide/messages';
import {
  SettingsCard,
  SettingsCardContent,
  SettingsCardFooter,
} from '../../_modules/settings-card';

type Detail = { label: string; value: string };

const DetailRow = ({ label, value }: Detail) => (
  <div className="flex w-full gap-4 leading-8">
    <div className="text-on-surface-variant w-48 shrink-0">{label}:</div>
    <div className="min-w-0 flex-1 break-all select-text">{value}</div>
  </div>
);

const DetailsButton = ({ details }: { details: Detail[] }) => (
  <Modal>
    <ModalTrigger asChild>
      <Button variant="flat">
        {m.settings_system_proxy_system_service_ctrl_detail()}
      </Button>
    </ModalTrigger>
    <ModalContent>
      <Card className="max-h-[80vh] w-[min(46rem,calc(100vw-2rem))]">
        <CardHeader>
          <ModalTitle>
            {m.settings_system_proxy_system_service_ctrl_detail()}
          </ModalTitle>
        </CardHeader>
        <CardContent className="max-h-[60vh] gap-1 overflow-auto">
          {details.map((detail) => (
            <DetailRow key={detail.label} {...detail} />
          ))}
        </CardContent>
        <CardFooter>
          <ModalClose>{m.common_close()}</ModalClose>
        </CardFooter>
      </Card>
    </ModalContent>
  </Modal>
);

export default function SystemServiceControl() {
  const coreStatusQuery = useCoreStatus();
  const promptDialog = useServerManualPromptDialog();
  const {
    query,
    isInstalled,
    installPending,
    controlPending,
    isBusy: serviceBusy,
    handleInstall,
    handleControl,
  } = useSystemServiceActions(promptDialog.show);
  const [refreshPending, startRefresh] = useTransition();
  const isBusy = serviceBusy || refreshPending;

  const serviceServer = query.data?.server;
  const runtimeInfos = serviceServer?.runtime_infos;
  const serviceCoreInfos = serviceServer?.core_infos;
  const serviceCoreType = getServiceCoreTypeLabel(serviceCoreInfos?.type);
  const details: Detail[] = [
    [m.settings_service_name_label(), query.data?.name],
    [m.settings_service_version_label(), query.data?.version],
    [m.settings_server_version_label(), serviceServer?.version],
    [
      m.settings_app_core_status_label(),
      getAppCoreStatusLabel(coreStatusQuery.data?.status),
    ],
    [m.settings_run_type_label(), coreStatusQuery.data?.type],
    [m.settings_core_type_label(), serviceCoreType],
    [m.settings_core_config_path_label(), serviceCoreInfos?.config_path],
    [m.settings_service_config_dir_label(), runtimeInfos?.service_config_dir],
    [m.settings_service_data_dir_label(), runtimeInfos?.service_data_dir],
    [m.settings_bound_config_dir_label(), runtimeInfos?.nyanpasu_config_dir],
    [m.settings_bound_data_dir_label(), runtimeInfos?.nyanpasu_data_dir],
  ].map(([label, value]) => ({
    label: String(label),
    value: String(value ?? m.common_unknown()),
  }));

  return (
    <>
      <ServerManualPromptDialogWrapper />
      <SettingsCard>
        <SettingsCardContent className="gap-1 py-4">
          <DetailRow
            label={m.settings_service_name_label()}
            value={query.data?.name || m.common_unknown()}
          />
          <DetailRow
            label={m.settings_server_version_label()}
            value={serviceServer?.version || m.common_unknown()}
          />
          <DetailRow
            label={m.common_current_status()}
            value={getSystemServiceStatusLabel(query.data?.status)}
          />
        </SettingsCardContent>
        <SettingsCardFooter className="flex-wrap-reverse gap-2">
          {isInstalled ? (
            <>
              <Button
                variant="flat"
                loading={controlPending}
                disabled={isBusy}
                onClick={handleControl}
              >
                {query.data?.status === 'running'
                  ? m.settings_system_proxy_system_service_ctrl_stop()
                  : m.settings_system_proxy_system_service_ctrl_start()}
              </Button>
              <Button
                loading={installPending}
                disabled={isBusy}
                onClick={handleInstall}
              >
                {m.settings_system_proxy_system_service_ctrl_uninstall()}
              </Button>
            </>
          ) : (
            <Button
              variant="flat"
              loading={installPending}
              disabled={isBusy}
              onClick={handleInstall}
            >
              {m.settings_system_proxy_system_service_ctrl_install()}
            </Button>
          )}
          <DetailsButton details={details} />
          <div className="flex-1" />
          <Button
            variant="flat"
            loading={refreshPending}
            disabled={isBusy}
            onClick={() =>
              startRefresh(async () => {
                await Promise.all([query.refetch(), coreStatusQuery.refetch()]);
              })
            }
          >
            {m.common_refresh()}
          </Button>
        </SettingsCardFooter>
      </SettingsCard>
    </>
  );
}
