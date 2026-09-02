import {
  useClashConfig,
  useClashCores,
  useCoreStatus,
  useSetting,
  useSystemProxy,
  useSystemService,
  type CoreState,
} from '@chimera/interface';
import { cn } from '@chimera/ui';
import { Link } from '@tanstack/react-router';
import { useMemo } from 'react';
import {
  SystemProxyButton,
  TunModeButton,
} from '@/components/settings/system-proxy';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardHeader } from '@/components/ui/card';
import TextMarquee from '@/components/ui/text-marquee';
import { getCoreStatusBadgeMessage } from '@/features/dashboard/core-service-status';
import useCoreIcon from '@/hooks/use-core-icon';
import * as m from '@/paraglide/messages';
import type { WidgetComponentProps } from './consts';
import WidgetItem from './widget-item';

enum ProxyStatus {
  SYSTEM = 'system',
  TUN = 'tun',
  OCCUPIED = 'occupied',
  DISABLED = 'disabled',
}

/** Resolve and render the current proxy mode badge. */
const ProxyTitleRow = () => {
  const { value: enableSystemProxy } = useSetting('enable_system_proxy');
  const { value: enableTunMode } = useSetting('enable_tun_mode');
  const { data: systemProxyStatus } = useSystemProxy();
  const {
    query: { data: clashConfigs },
  } = useClashConfig();

  const status = useMemo<ProxyStatus>(() => {
    if (enableTunMode) {
      return ProxyStatus.TUN;
    }

    if (enableSystemProxy && systemProxyStatus?.enable) {
      const port = Number(systemProxyStatus.server.split(':')[1]);

      if (port === clashConfigs?.['mixed-port']) {
        return ProxyStatus.SYSTEM;
      }

      return ProxyStatus.OCCUPIED;
    }

    return ProxyStatus.DISABLED;
  }, [enableSystemProxy, enableTunMode, systemProxyStatus, clashConfigs]);

  const messages = {
    [ProxyStatus.SYSTEM]: m.dashboard_widget_proxy_status_success_system(),
    [ProxyStatus.TUN]: m.dashboard_widget_proxy_status_success_tun(),
    [ProxyStatus.OCCUPIED]: m.dashboard_widget_proxy_status_occupied(),
    [ProxyStatus.DISABLED]: m.dashboard_widget_proxy_status_disabled(),
  };

  return (
    <CardHeader className="flex items-center gap-3">
      <span className="shrink-0 font-bold">
        {m.dashboard_widget_proxy_status()}
      </span>

      <Button
        variant="raised"
        className={cn(
          'flex h-6 min-w-0 items-center px-0',
          status === ProxyStatus.DISABLED &&
            'bg-secondary-container hover:bg-on-secondary',
          status === ProxyStatus.OCCUPIED &&
            'bg-error-container hover:bg-on-error',
          status === ProxyStatus.SYSTEM &&
            'bg-primary-container hover:bg-on-primary',
          status === ProxyStatus.TUN &&
            'bg-tertiary-container hover:bg-on-tertiary',
        )}
        asChild
      >
        <Link to="/main/settings/system">
          <TextMarquee className="px-2" fadeEdges fadeWidth={8}>
            {messages[status]}
          </TextMarquee>
        </Link>
      </Button>
    </CardHeader>
  );
};

/** Render the system proxy and TUN shortcut controls. */
export function ProxyShortcutsWidget({
  id,
  onCloseClick,
}: WidgetComponentProps) {
  return (
    <WidgetItem id={id} minW={3} minH={2} onCloseClick={onCloseClick}>
      <Card className="flex size-full flex-col justify-between">
        <ProxyTitleRow />

        <CardContent className="flex-1 gap-3">
          <SystemProxyButton className="h-full rounded-3xl" />
          <TunModeButton className="h-full rounded-3xl" />
        </CardContent>
      </Card>
    </WidgetItem>
  );
}

/** Build the explanatory badge for the current core/service state. */
const CoreStatusBadge = () => {
  const {
    query: { data: serviceStatus },
  } = useSystemService();
  const coreStatusQuery = useCoreStatus();

  const message = useMemo<string>(
    () =>
      getCoreStatusBadgeMessage({
        coreState: coreStatusQuery.data?.status,
        serviceStatus: serviceStatus?.status,
        serviceCoreState: serviceStatus?.server?.core_infos?.state,
      }),
    [serviceStatus, coreStatusQuery.data],
  );

  return (
    <div
      className={cn(
        'flex h-6 min-w-0 items-center rounded-full text-sm',
        'bg-surface-variant/50',
      )}
      data-slot="core-status-badge"
    >
      <TextMarquee className="px-2" fadeEdges fadeWidth={8}>
        {message}
      </TextMarquee>
    </div>
  );
};

/** Render the selected core, version, and live status. */
const CurrentCoreCard = () => {
  const { query: clashCores } = useClashCores();
  const { value: currentCoreKey } = useSetting('clash_core');
  const currentCoreIcon = useCoreIcon(currentCoreKey);
  const currentCore = currentCoreKey && clashCores.data?.[currentCoreKey];
  const coreStatusQuery = useCoreStatus();
  const coreState = coreStatusQuery.data?.status as CoreState | undefined;
  const isRunning = coreState === 'Running';

  return (
    <Button
      variant="raised"
      className={cn(
        'group flex flex-1 items-center gap-4 rounded-2xl pr-3 pl-4',
        'bg-surface-variant/30 hover:bg-surface-variant',
      )}
      data-running={String(isRunning)}
      data-slot="current-core-card"
      asChild
    >
      <Link to="/main/settings/clash">
        <img
          src={currentCoreIcon}
          alt={currentCore?.name ?? currentCoreKey ?? ''}
          className="size-12 shrink-0"
          data-slot="core-icon"
        />

        <div
          className="flex flex-1 flex-col items-start gap-1 truncate"
          data-slot="core-info"
        >
          <div className="font-semibold" data-slot="core-name">
            {currentCore?.name ?? currentCoreKey ?? '-'}
          </div>

          <div
            className="text-zinc-700 dark:text-zinc-300"
            data-slot="core-version"
          >
            {currentCore?.currentVersion ?? '-'}
          </div>
        </div>

        <div
          className="flex items-center gap-2 truncate pr-2"
          data-slot="core-status"
        >
          <div className="truncate" data-slot="core-status-text">
            {isRunning
              ? m.dashboard_widget_core_status_running()
              : m.dashboard_widget_core_status_stopped()}
          </div>

          <div
            className="relative flex size-3 shrink-0"
            data-slot="core-status-indicator"
          >
            <span
              className={cn(
                'absolute inline-flex size-full animate-ping rounded-full opacity-75',
                'group-data-[running=true]:bg-green-500',
                'group-data-[running=false]:opacity-0',
              )}
            />
            <span
              className={cn(
                'relative inline-flex size-full rounded-full',
                'group-data-[running=true]:bg-green-500',
                'group-data-[running=false]:bg-gray-400',
              )}
            />
          </div>
        </div>
      </Link>
    </Button>
  );
};

/** Render the core status shortcut card. */
export function CoreShortcutsWidget({
  id,
  onCloseClick,
}: WidgetComponentProps) {
  return (
    <WidgetItem id={id} minW={4} minH={2} onCloseClick={onCloseClick}>
      <Card className="flex size-full flex-col justify-between">
        <CardHeader>
          <span className="shrink-0 font-bold">
            {m.dashboard_widget_core_status()}
          </span>
          <CoreStatusBadge />
        </CardHeader>

        <CardContent className="flex-1">
          <CurrentCoreCard />
        </CardContent>
      </Card>
    </WidgetItem>
  );
}
