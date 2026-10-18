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
import { useDndGridContext } from '@chimera/ui/dnd-grid';
import { Link } from '@tanstack/react-router';
import NetworkPingRounded from '~icons/material-symbols/network-ping-rounded';
import SettingsEthernetRounded from '~icons/material-symbols/settings-ethernet-rounded';
import { useMemo } from 'react';
import {
  SystemProxyButton,
  TunModeButton,
} from '@/components/settings/system-proxy';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardHeader } from '@/components/ui/card';
import TextMarquee from '@/components/ui/text-marquee';
import { getCoreStatusBadgeMessage } from '@/features/dashboard/core-service-status';
import { getProxyStatus } from '@/features/dashboard/proxy-status';
import useCoreIcon from '@/hooks/use-core-icon';
import * as m from '@/paraglide/messages';
import type { WidgetComponentProps } from './consts';
import { useWidgetConfig } from './provider';
import { WidgetId, type WidgetConfigs } from './widget-config';
import WidgetItem from './widget-item';

/** Resolve and render the current proxy mode badge. */
const ProxyTitleRow = ({
  enableSystemProxy,
  enableTunMode,
}: {
  enableSystemProxy: boolean | undefined;
  enableTunMode: boolean | undefined;
}) => {
  const { data: systemProxyStatus } = useSystemProxy();
  const {
    query: { data: clashConfigs },
  } = useClashConfig();

  const status = useMemo(
    () =>
      getProxyStatus({
        enableSystemProxy,
        enableTunMode,
        systemProxyEnabled: systemProxyStatus?.enable,
        systemProxyServer: systemProxyStatus?.server,
        mixedPort: clashConfigs?.['mixed-port'],
      }),
    [enableSystemProxy, enableTunMode, systemProxyStatus, clashConfigs],
  );

  const messages = {
    system: m.dashboard_widget_proxy_status_success_system(),
    tun: m.dashboard_widget_proxy_status_success_tun(),
    occupied: m.dashboard_widget_proxy_status_occupied(),
    disabled: m.dashboard_widget_proxy_status_disabled(),
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
          status === 'disabled' &&
            'bg-secondary-container hover:bg-on-secondary',
          status === 'occupied' && 'bg-error-container hover:bg-on-error',
          status === 'system' && 'bg-primary-container hover:bg-on-primary',
          status === 'tun' && 'bg-tertiary-container hover:bg-on-tertiary',
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
  const config = useWidgetConfig(id, WidgetId.ProxyShortcuts);
  const { value: enableSystemProxy } = useSetting('enable_system_proxy');
  const { value: enableTunMode } = useSetting('enable_tun_mode');
  const { sourceOnly, isOverlay } = useDndGridContext();
  const isPreview = sourceOnly || isOverlay;
  const buttons =
    config.order === 'system-first' ? ['system', 'tun'] : ['tun', 'system'];

  return (
    <WidgetItem
      id={id}
      widgetType={WidgetId.ProxyShortcuts}
      minW={3}
      minH={2}
      onCloseClick={onCloseClick}
    >
      <Card className="flex size-full flex-col justify-between">
        <ProxyTitleRow
          enableSystemProxy={enableSystemProxy ?? undefined}
          enableTunMode={enableTunMode ?? undefined}
        />

        <CardContent
          className="flex-1 flex-row items-stretch gap-2"
          data-slot="proxy-shortcut-buttons"
        >
          {buttons
            .filter(
              (button) =>
                config.buttons === 'both' || config.buttons === button,
            )
            .map((button) => {
              const isSystem = button === 'system';
              const isActive = isPreview
                ? isSystem
                : isSystem
                  ? Boolean(enableSystemProxy)
                  : Boolean(enableTunMode);
              const className =
                'h-full min-w-0 flex-1 basis-0 rounded-3xl px-2 transition-[flex-grow] duration-300 motion-reduce:transition-none';
              const style = {
                flexGrow:
                  config.buttons === 'both' &&
                  config.layout === 'flex' &&
                  isActive
                    ? 1.65
                    : 1,
              };
              if (isPreview) {
                return (
                  <Button
                    key={button}
                    variant="raised"
                    disabled
                    className={className}
                    style={style}
                  >
                    {isSystem ? (
                      <NetworkPingRounded />
                    ) : (
                      <SettingsEthernetRounded />
                    )}
                    <span>
                      {isSystem
                        ? m.settings_system_proxy_system_proxy_label()
                        : m.settings_system_proxy_tun_mode_label()}
                    </span>
                  </Button>
                );
              }
              return button === 'system' ? (
                <SystemProxyButton
                  key={button}
                  className={className}
                  style={style}
                />
              ) : (
                <TunModeButton
                  key={button}
                  className={className}
                  style={style}
                />
              );
            })}
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
const CurrentCoreCard = ({
  config,
}: {
  config: WidgetConfigs[WidgetId.CoreShortcuts];
}) => {
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
        'group grid h-auto min-w-0 flex-1 gap-2 rounded-2xl px-3 py-2 text-left',
        'grid-rows-1',
        'bg-surface-variant/30 hover:bg-surface-variant',
      )}
      data-running={String(isRunning)}
      data-slot="current-core-card"
      asChild
    >
      <Link to="/main/settings/clash">
        <div
          className="flex w-full min-w-0 items-center gap-3"
          data-slot="current-core-info"
        >
          <img
            src={currentCoreIcon}
            alt={currentCore?.name ?? currentCoreKey ?? ''}
            className="size-12 shrink-0"
            data-slot="core-icon"
          />
          <div
            className="flex min-w-0 flex-1 flex-col items-start gap-1 truncate"
            data-slot="core-info"
          >
            <div className="truncate font-semibold" data-slot="core-name">
              {currentCore?.name ?? currentCoreKey ?? '-'}
            </div>
            {config.showVersion && (
              <div
                className="truncate text-xs text-zinc-700 dark:text-zinc-300"
                data-slot="core-version"
              >
                {currentCore?.currentVersion ?? '-'}
              </div>
            )}
          </div>
          <div
            className="flex shrink-0 items-center gap-2 truncate"
            data-slot="core-status"
          >
            <span className="truncate text-xs" data-slot="core-status-text">
              {isRunning
                ? m.dashboard_widget_core_status_running()
                : m.dashboard_widget_core_status_stopped()}
            </span>
            <span
              className={cn(
                'size-3 shrink-0 rounded-full',
                isRunning ? 'bg-green-500' : 'bg-gray-400',
              )}
              data-slot="core-status-indicator"
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
  const config = useWidgetConfig(id, WidgetId.CoreShortcuts);
  return (
    <WidgetItem
      id={id}
      widgetType={WidgetId.CoreShortcuts}
      minW={4}
      minH={2}
      onCloseClick={onCloseClick}
    >
      <Card className="flex size-full flex-col justify-between">
        <CardHeader>
          <span className="shrink-0 font-bold">
            {m.dashboard_widget_core_status()}
          </span>
          <CoreStatusBadge />
        </CardHeader>

        <CardContent className="flex-1">
          <CurrentCoreCard config={config} />
        </CardContent>
      </Card>
    </WidgetItem>
  );
}
