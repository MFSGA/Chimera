import {
  isTauri,
  useClashConfig,
  useClashCores,
  useClashSetting,
  useCoreActorStatus,
  useCoreStatus,
  useSetting,
  useSystemProxy,
  useSystemService,
} from '@chimera/interface';
import { cn } from '@chimera/ui';
import { Button } from '@chimera/ui/button';
import { Card, CardContent } from '@chimera/ui/card';
import { useDndGridContext } from '@chimera/ui/dnd-grid';
import { ShapeToggle } from '@chimera/ui/shape-toggle';
import TextMarquee from '@chimera/ui/text-marquee';
import { Link } from '@tanstack/react-router';
import ChevronRightRounded from '~icons/material-symbols/chevron-right-rounded';
import MemoryOutlineRounded from '~icons/material-symbols/memory-outline-rounded';
import NetworkPingRounded from '~icons/material-symbols/network-ping-rounded';
import SettingsEthernetRounded from '~icons/material-symbols/settings-ethernet-rounded';
import ShieldRounded from '~icons/material-symbols/shield-rounded';
import { useMemo } from 'react';
import {
  SystemProxyButton,
  TransparentProxyButton,
  TunModeButton,
} from '@/components/settings/system-proxy';
import { OS } from '@/consts';
import {
  getCoreStatusBadgeMessage,
  getRoutedCoreStatusMessage,
  isRoutedCoreRunning,
} from '@/features/dashboard/core-service-status';
import { getProxyStatus } from '@/features/dashboard/proxy-status';
import { useTransparentProxyAction } from '@/features/system-proxy/use-proxy-settings';
import useCoreIcon from '@/hooks/use-core-icon';
import * as m from '@/paraglide/messages';
import type { WidgetComponentProps } from './consts';
import { coreControllerChannel } from './dashboard-core-channel';
import { useWidgetConfig } from './provider';
import { WidgetId, type WidgetConfigs } from './widget-config';
import WidgetItem from './widget-item';
import { WidgetHeader, WidgetTitle } from './widget-ui';

/** Resolve the actual network state; an unapplied transparent config is not active. */
const ProxyTitleRow = ({
  enableSystemProxy,
  enableTunMode,
}: {
  enableSystemProxy: boolean | undefined;
  enableTunMode: boolean | undefined;
}) => {
  const { data: systemProxyStatus } = useSystemProxy();
  const transparent = useTransparentProxyAction();
  const {
    query: { data: clashConfigs },
  } = useClashConfig();

  const status = useMemo(
    () =>
      getProxyStatus({
        enableSystemProxy,
        enableTunMode,
        transparentProxyActive:
          transparent.visible &&
          transparent.status?.active === true &&
          transparent.status.mode === transparent.mode,
        systemProxyEnabled: systemProxyStatus?.enable,
        systemProxyServer: systemProxyStatus?.server,
        mixedPort: clashConfigs?.['mixed-port'],
      }),
    [
      enableSystemProxy,
      enableTunMode,
      transparent.visible,
      transparent.status,
      transparent.mode,
      systemProxyStatus,
      clashConfigs,
    ],
  );

  const messages = {
    system: m.dashboard_widget_proxy_status_success_system(),
    tun: m.dashboard_widget_proxy_status_success_tun(),
    transparent: m.settings_clash_transparent_proxy_label(),
    occupied: m.dashboard_widget_proxy_status_occupied(),
    disabled: m.dashboard_widget_proxy_status_disabled(),
  };

  return (
    <WidgetHeader>
      <WidgetTitle icon={SettingsEthernetRounded}>
        {m.dashboard_widget_proxy_status()}
      </WidgetTitle>
      <Button
        variant="raised"
        className={cn(
          'flex h-6 min-w-0 items-center px-0',
          status === 'disabled' &&
            'bg-secondary-container hover:bg-on-secondary',
          status === 'occupied' && 'bg-error-container hover:bg-on-error',
          status === 'system' && 'bg-primary-container hover:bg-on-primary',
          (status === 'tun' || status === 'transparent') &&
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
    </WidgetHeader>
  );
};

export function ProxyShortcutsWidget({
  id,
  onCloseClick,
}: WidgetComponentProps) {
  const config = useWidgetConfig(id, WidgetId.ProxyShortcuts);
  const { value: enableSystemProxy } = useSetting('enable_system_proxy');
  const { value: enableTunMode } = useClashSetting('enable_tun_mode');
  const { sourceOnly, isOverlay } = useDndGridContext();
  const preview = sourceOnly || isOverlay;
  const linuxDesktop = isTauri() && OS === 'linux';
  const buttons: ('system' | 'tun' | 'transparent')[] = [
    ...(config.order === 'system-first'
      ? (['system', 'tun'] as const)
      : (['tun', 'system'] as const)),
    'transparent',
  ];

  return (
    <WidgetItem
      id={id}
      widgetType={WidgetId.ProxyShortcuts}
      minW={3}
      minH={2}
      onCloseClick={onCloseClick}
    >
      <Card className="flex size-full flex-col justify-between">
        {preview ? (
          <WidgetHeader>
            <WidgetTitle icon={SettingsEthernetRounded}>
              {m.dashboard_widget_proxy_status()}
            </WidgetTitle>
            <Button
              variant="raised"
              disabled
              className="bg-primary-container text-on-primary-container flex h-6 min-w-0 items-center px-2 text-xs"
            >
              {config.buttons.length === 1 && config.buttons[0] === 'tun'
                ? m.dashboard_widget_proxy_status_success_tun()
                : config.buttons.length === 1 &&
                    config.buttons[0] === 'transparent'
                  ? m.settings_clash_transparent_proxy_label()
                  : m.dashboard_widget_proxy_status_success_system()}
            </Button>
          </WidgetHeader>
        ) : (
          <ProxyTitleRow
            enableSystemProxy={enableSystemProxy}
            enableTunMode={enableTunMode}
          />
        )}
        <CardContent
          className="min-h-0 flex-1 flex-row items-stretch justify-center gap-2 overflow-hidden py-3"
          data-slot="proxy-shortcut-buttons"
        >
          {buttons
            .filter(
              (button) =>
                config.buttons.includes(button) &&
                (button !== 'transparent' || linuxDesktop),
            )
            .map((button) => {
              const content = preview ? (
                <ShapeToggle
                  disabled
                  active={button === 'system'}
                  elastic={config.layout === 'flex'}
                  icon={
                    button === 'system' ? (
                      <NetworkPingRounded />
                    ) : button === 'tun' ? (
                      <SettingsEthernetRounded />
                    ) : (
                      <ShieldRounded />
                    )
                  }
                  label={
                    button === 'system'
                      ? m.settings_system_proxy_system_proxy_label()
                      : button === 'tun'
                        ? m.settings_system_proxy_tun_mode_label()
                        : m.settings_clash_transparent_proxy_label()
                  }
                />
              ) : button === 'system' ? (
                <SystemProxyButton
                  presentation="cookie"
                  elastic={config.layout === 'flex'}
                />
              ) : button === 'tun' ? (
                <TunModeButton
                  presentation="cookie"
                  elastic={config.layout === 'flex'}
                />
              ) : (
                <TransparentProxyButton elastic={config.layout === 'flex'} />
              );
              return (
                <div key={button} className="flex min-w-0 flex-1">
                  {content}
                </div>
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
  const routedCoreStatusQuery = useCoreActorStatus();
  const coreStatusQuery = useCoreStatus({
    enabled: !routedCoreStatusQuery.data,
  });

  const message = useMemo<string>(
    () =>
      routedCoreStatusQuery.data
        ? getRoutedCoreStatusMessage(routedCoreStatusQuery.data)
        : !coreStatusQuery.data
          ? m.common_loading()
          : getCoreStatusBadgeMessage({
              coreState: coreStatusQuery.data?.status,
              serviceStatus: serviceStatus?.status,
              serviceCoreState: serviceStatus?.server?.core_infos?.state,
            }),
    [serviceStatus, coreStatusQuery.data, routedCoreStatusQuery.data],
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

/** Ref core card layout with Chimera's authoritative CoreActor state. */
const CurrentCoreCard = ({
  config,
}: {
  config: WidgetConfigs[WidgetId.CoreShortcuts];
}) => {
  const { query: clashCores } = useClashCores();
  const { value: currentCoreKey } = useSetting('core');
  const currentCoreIcon = useCoreIcon(currentCoreKey);
  const currentCore = currentCoreKey && clashCores.data?.[currentCoreKey];
  const actorStatusQuery = useCoreActorStatus();
  const coreStatusQuery = useCoreStatus({ enabled: !actorStatusQuery.data });
  const actorStatus = actorStatusQuery.data;
  const channel = coreControllerChannel(actorStatus?.controller);
  const isRunning = actorStatus
    ? isRoutedCoreRunning(actorStatus)
    : coreStatusQuery.data?.status === 'Running';
  const displayStatus = actorStatus
    ? getRoutedCoreStatusMessage(actorStatus)
    : !coreStatusQuery.data
      ? m.common_loading()
      : isRunning
        ? m.dashboard_widget_core_status_running()
        : m.dashboard_widget_core_status_stopped();

  return (
    <Button
      variant="raised"
      className={cn(
        'group grid h-auto min-w-0 flex-1 rounded-[20px] px-2.5 py-0 text-left',
        config.showChannel
          ? 'grid-rows-[minmax(3.5rem,1fr)_minmax(2rem,0.6fr)]'
          : 'grid-rows-1',
        'bg-surface-variant/30 text-on-surface hover:bg-surface-variant/50 shadow-none hover:shadow-none focus:shadow-none',
        'focus-visible:outline-primary focus-visible:outline-2 focus-visible:-outline-offset-2',
      )}
      data-running={String(isRunning)}
      data-slot="current-core-card"
      asChild
    >
      <Link to="/main/settings/clash">
        <div
          className="flex w-full min-w-0 shrink-0 items-center gap-3"
          data-slot="current-core-info"
        >
          <div className="bg-surface/60 grid size-10 shrink-0 place-items-center rounded-xl">
            <img
              src={currentCoreIcon}
              alt=""
              className="size-8 object-contain"
              data-slot="core-icon"
            />
          </div>
          <div className="min-w-0 flex-1" data-slot="core-info">
            <div
              className="flex min-w-0 items-center gap-2 text-base leading-5 font-semibold"
              title={currentCore?.name || undefined}
              data-slot="core-name"
            >
              <TextMarquee className="min-w-0">
                {currentCore?.name || '—'}
              </TextMarquee>
            </div>
            {config.showVersion && (
              <div
                className="text-on-surface-variant truncate text-xs leading-4 font-normal"
                title={currentCore?.currentVersion || undefined}
                data-slot="core-version"
              >
                <TextMarquee className="w-full">
                  {currentCore?.currentVersion ?? '—'}
                </TextMarquee>
              </div>
            )}
          </div>
          <div
            className={cn(
              'flex shrink-0 items-center gap-1.5 rounded-full px-2.5 py-1 text-xs font-medium',
              isRunning
                ? 'bg-primary-container text-on-primary-container'
                : 'bg-surface-variant text-on-surface-variant',
            )}
            data-slot="core-status"
          >
            <span
              className="size-1.5 shrink-0 rounded-full bg-current"
              aria-hidden="true"
              data-slot="core-status-indicator"
            />
            <span data-slot="core-status-text">{displayStatus}</span>
          </div>
        </div>
        {config.showChannel && (
          <div
            className="border-outline-variant/40 text-on-surface-variant flex w-full min-w-0 items-center gap-2 border-t text-xs leading-4 font-normal"
            data-slot="core-control-channel"
          >
            <span className="min-w-0 truncate">
              {m.settings_clash_control_channel_label()}
            </span>
            <span className="text-on-surface ml-auto shrink-0 font-medium">
              {channel}
            </span>
            <ChevronRightRounded
              className="size-4 shrink-0"
              aria-hidden="true"
            />
          </div>
        )}
      </Link>
    </Button>
  );
};

/** The ref's inert preview: no system/CoreActor requests while dragging a
 * WidgetSheet source or rendering the overlay. */
const CoreShortcutPreview = ({
  config,
}: {
  config: WidgetConfigs[WidgetId.CoreShortcuts];
}) => {
  const icon = useCoreIcon('mihomo');
  return (
    <Button
      variant="raised"
      disabled
      className={cn(
        'grid h-full w-full min-w-0 rounded-[20px] px-2.5 py-0 text-left',
        config.showChannel
          ? 'grid-rows-[minmax(3.5rem,1fr)_minmax(2rem,0.6fr)]'
          : 'grid-rows-1',
        'bg-surface-variant/30 text-on-surface shadow-none',
      )}
      data-running="true"
      data-slot="current-core-card"
    >
      <div className="flex w-full min-w-0 items-center gap-3">
        <div className="bg-surface/60 grid size-10 shrink-0 place-items-center rounded-xl">
          <img
            src={icon}
            alt=""
            className="size-8 object-contain"
            data-slot="core-icon"
          />
        </div>
        <div className="min-w-0 flex-1" data-slot="core-info">
          <div
            className="truncate text-base leading-5 font-semibold"
            data-slot="core-name"
          >
            Mihomo
          </div>
          {config.showVersion && (
            <div
              className="text-on-surface-variant truncate text-xs leading-4"
              data-slot="core-version"
            >
              v1.19.12
            </div>
          )}
        </div>
        <div
          className="bg-primary-container text-on-primary-container flex shrink-0 items-center gap-1.5 rounded-full px-2.5 py-1 text-xs font-medium"
          data-slot="core-status"
        >
          <span
            className="size-1.5 shrink-0 rounded-full bg-current"
            aria-hidden="true"
            data-slot="core-status-indicator"
          />
          <span data-slot="core-status-text">
            {m.dashboard_widget_core_status_running()}
          </span>
        </div>
      </div>
      {config.showChannel && (
        <div
          className="border-outline-variant/40 text-on-surface-variant flex w-full min-w-0 items-center gap-2 border-t text-xs leading-4"
          data-slot="core-control-channel"
        >
          <span className="min-w-0 truncate">
            {m.settings_clash_control_channel_label()}
          </span>
          <span className="text-on-surface ml-auto shrink-0 font-medium">
            HTTP
          </span>
          <ChevronRightRounded className="size-4 shrink-0" aria-hidden="true" />
        </div>
      )}
    </Button>
  );
};

/** Render the core status shortcut card. */
export function CoreShortcutsWidget({
  id,
  onCloseClick,
}: WidgetComponentProps) {
  const config = useWidgetConfig(id, WidgetId.CoreShortcuts);
  const { sourceOnly, isOverlay } = useDndGridContext();
  const preview = sourceOnly || isOverlay;
  return (
    <WidgetItem
      id={id}
      widgetType={WidgetId.CoreShortcuts}
      minW={4}
      minH={2}
      onCloseClick={onCloseClick}
    >
      <Card className="flex size-full flex-col justify-between">
        <WidgetHeader>
          <WidgetTitle icon={MemoryOutlineRounded}>
            {m.dashboard_widget_core_status()}
          </WidgetTitle>
          {preview ? (
            <div
              className="bg-primary-container text-on-primary-container flex h-6 max-w-full min-w-0 items-center rounded-full px-2 text-xs font-medium"
              data-slot="core-status-badge"
            >
              {m.dashboard_widget_core_status_running_by_child_process()}
            </div>
          ) : (
            <CoreStatusBadge />
          )}
        </WidgetHeader>
        <CardContent className="min-h-0 flex-1 pt-2 pb-3">
          {preview ? (
            <CoreShortcutPreview config={config} />
          ) : (
            <CurrentCoreCard config={config} />
          )}
        </CardContent>
      </Card>
    </WidgetItem>
  );
}
