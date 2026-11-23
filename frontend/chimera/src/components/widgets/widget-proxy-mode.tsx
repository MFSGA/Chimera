import {
  MutationUnconfirmedError,
  useClashConfig,
  useCoreActorStatus,
  useCoreStatus,
  type ProxyMode,
} from '@chimera/interface';
import { Button } from '@chimera/ui/button';
import { Card, CardContent } from '@chimera/ui/card';
import { useDndGridContext } from '@chimera/ui/dnd-grid';
import { ExpressiveChoice } from '@chimera/ui/expressive-choice';
import { Tooltip, TooltipContent, TooltipTrigger } from '@chimera/ui/tooltip';
import NorthEastRounded from '~icons/material-symbols/north-east-rounded';
import PublicRounded from '~icons/material-symbols/public-rounded';
import RefreshRounded from '~icons/material-symbols/refresh-rounded';
import RouteRounded from '~icons/material-symbols/route-rounded';
import { useState } from 'react';
import { isRoutedCoreRunning } from '@/features/dashboard/core-service-status';
import { isWidgetQueryReconciled } from '@/features/dashboard/widget-query-reconciliation';
import * as m from '@/paraglide/messages';
import type { WidgetComponentProps } from './consts';
import { useWidgetConfig } from './provider';
import { WidgetId } from './widget-config';
import WidgetItem from './widget-item';
import { WidgetHeader, WidgetTitle } from './widget-ui';

// The ref widget intentionally exposes the three routing modes, not Clash
// Premium's legacy script mode supported elsewhere in Chimera.
type RoutingMode = Extract<ProxyMode, 'rule' | 'global' | 'direct'>;
const MODE_KEYS: RoutingMode[] = ['rule', 'global', 'direct'];
const MODE_ICONS = {
  rule: RouteRounded,
  global: PublicRounded,
  direct: NorthEastRounded,
};

function modeLabel(mode: RoutingMode): string {
  switch (mode) {
    case 'rule':
      return m.dashboard_widget_proxy_mode_rule();
    case 'global':
      return m.dashboard_widget_proxy_mode_global();
    case 'direct':
      return m.dashboard_widget_proxy_mode_direct();
  }
}

function modeDescription(mode: RoutingMode): string {
  switch (mode) {
    case 'rule':
      return m.dashboard_widget_proxy_mode_rule_description();
    case 'global':
      return m.dashboard_widget_proxy_mode_global_description();
    case 'direct':
      return m.dashboard_widget_proxy_mode_direct_description();
  }
}

function ProxyModeOptions({
  modes,
  currentMode,
  disabled,
  onChange,
  layout = 'focus',
}: {
  modes: RoutingMode[];
  currentMode: RoutingMode | null;
  disabled: boolean;
  onChange?: (mode: RoutingMode) => void;
  layout?: 'focus' | 'flex';
}) {
  return (
    <ExpressiveChoice
      label={m.dashboard_widget_proxy_mode_title()}
      options={modes.map((mode) => {
        const Icon = MODE_ICONS[mode];
        return {
          value: mode,
          label: modeLabel(mode),
          description: modeDescription(mode),
          icon: <Icon aria-hidden />,
        };
      })}
      value={currentMode}
      layout={layout}
      disabled={disabled}
      onChange={(value) => {
        const mode = modes.find((mode) => mode === value);
        if (mode) onChange?.(mode);
      }}
    />
  );
}

function ProxyModePreview({ id }: { id: string }) {
  const config = useWidgetConfig(id, WidgetId.ProxyMode);
  return (
    <WidgetItem id={id} widgetType={WidgetId.ProxyMode} minW={4} minH={2}>
      <Card className="flex size-full flex-col">
        <WidgetHeader className="gap-2">
          <WidgetTitle icon={RouteRounded}>
            {m.dashboard_widget_proxy_mode_title()}
          </WidgetTitle>
        </WidgetHeader>
        <CardContent className="min-h-0 flex-1 justify-start gap-1 overflow-hidden px-4 py-3">
          <ProxyModeOptions
            modes={MODE_KEYS}
            currentMode="rule"
            disabled
            layout={config.layout}
          />
        </CardContent>
      </Card>
    </WidgetItem>
  );
}

type ActionState = 'idle' | 'degraded' | 'unconfirmed' | 'failed';

function ProxyModeLive({
  id,
  onCloseClick,
  disabled,
}: WidgetComponentProps & {
  disabled: boolean;
}) {
  // Chimera exposes the underlying runtime config mutation, not ref's
  // proxyMode wrapper. Keep the server-confirmed config as the source of truth.
  const proxyMode = useClashConfig();
  const config = useWidgetConfig(id, WidgetId.ProxyMode);
  const actorStatus = useCoreActorStatus();
  const coreStatus = useCoreStatus({ enabled: !actorStatus.data });
  const [actionState, setActionState] = useState<ActionState>('idle');
  const [checkedAfterUnconfirmed, setCheckedAfterUnconfirmed] = useState(false);
  const [checkingStatus, setCheckingStatus] = useState(false);
  const [checkFailed, setCheckFailed] = useState(false);
  const configData = proxyMode.query.data;
  const layout = config.layout;
  const rawMode = configData?.mode?.toLowerCase();
  const currentMode = MODE_KEYS.find((mode) => mode === rawMode) ?? null;
  // Wait for the authoritative CoreActor snapshot before allowing a write.
  // The legacy core query is a fallback only after that query settles.
  const waiting = proxyMode.query.isPending || actorStatus.isPending;
  const isOffline = actorStatus.data
    ? !isRoutedCoreRunning(actorStatus.data)
    : coreStatus.data != null && coreStatus.data.status !== 'Running';
  const loadFailed =
    (proxyMode.query.isError || (!actorStatus.data && coreStatus.isError)) &&
    !waiting;
  const actionable =
    !disabled &&
    (actionState !== 'unconfirmed' || checkedAfterUnconfirmed) &&
    !checkingStatus &&
    !waiting &&
    !loadFailed &&
    !isOffline &&
    configData !== undefined &&
    (actorStatus.data
      ? isRoutedCoreRunning(actorStatus.data)
      : coreStatus.data?.status === 'Running');

  const changeMode = async (mode: RoutingMode) => {
    if (!actionable || proxyMode.upsert.isPending || mode === currentMode) {
      return;
    }
    setActionState('idle');
    setCheckedAfterUnconfirmed(false);
    setCheckFailed(false);
    try {
      // Chimera's patchRuntimeOverrides command returns null on a confirmed
      // successful reply, unlike ref's richer MutationOutcome receipt.
      await proxyMode.upsert.mutateAsync({ mode });
      setActionState('idle');
    } catch (error) {
      if (error instanceof MutationUnconfirmedError) {
        setActionState('unconfirmed');
        setCheckingStatus(true);
        try {
          const result = await proxyMode.query.refetch();
          setCheckFailed(!isWidgetQueryReconciled(result));
        } catch {
          setCheckFailed(true);
        } finally {
          setCheckingStatus(false);
        }
      } else {
        setActionState('failed');
      }
    }
  };

  const checkStatus = async () => {
    if (disabled || checkingStatus) return;
    setCheckingStatus(true);
    setCheckFailed(false);
    try {
      const result = await proxyMode.query.refetch();
      if (!isWidgetQueryReconciled(result)) {
        setCheckFailed(true);
        setCheckedAfterUnconfirmed(false);
      } else {
        setCheckedAfterUnconfirmed(true);
      }
    } catch {
      setCheckFailed(true);
      setCheckedAfterUnconfirmed(false);
    } finally {
      setCheckingStatus(false);
    }
  };

  const message = waiting
    ? m.dashboard_widget_proxy_mode_loading()
    : isOffline
      ? m.dashboard_widget_proxy_mode_core_offline()
      : loadFailed
        ? m.dashboard_widget_proxy_mode_load_failed()
        : currentMode
          ? null
          : m.dashboard_widget_proxy_mode_unknown();

  return (
    <WidgetItem
      id={id}
      widgetType={WidgetId.ProxyMode}
      minW={4}
      minH={2}
      onCloseClick={onCloseClick}
    >
      <Card className="flex size-full flex-col" data-slot="proxy-mode-card">
        <WidgetHeader className="flex-wrap gap-x-2 gap-y-1">
          <WidgetTitle className="flex-1" icon={RouteRounded}>
            {m.dashboard_widget_proxy_mode_title()}
          </WidgetTitle>

          {actionState === 'unconfirmed' && (
            <>
              <Tooltip>
                <TooltipTrigger asChild>
                  <Button
                    variant="basic"
                    icon
                    className="ml-auto size-7 shrink-0"
                    aria-label={m.dashboard_widget_operation_check()}
                    disabled={disabled || checkingStatus}
                    loading={checkingStatus}
                    onClick={checkStatus}
                  >
                    <RefreshRounded className="size-4" />
                  </Button>
                </TooltipTrigger>
                <TooltipContent>
                  {m.dashboard_widget_operation_check()}
                </TooltipContent>
              </Tooltip>
            </>
          )}
          {message && (
            <span
              className={
                loadFailed || isOffline
                  ? 'text-error text-xs'
                  : 'text-on-surface-variant text-xs'
              }
              role={loadFailed || isOffline ? 'alert' : 'status'}
            >
              {message}
            </span>
          )}
        </WidgetHeader>

        <CardContent className="min-h-0 flex-1 justify-start gap-1 overflow-hidden px-4 py-3">
          <ProxyModeOptions
            modes={MODE_KEYS}
            currentMode={currentMode}
            layout={layout}
            disabled={!actionable || proxyMode.upsert.isPending}
            onChange={changeMode}
          />

          {actionState !== 'idle' && (
            <p
              className={
                actionState === 'failed'
                  ? 'text-error text-xs'
                  : 'text-on-surface-variant text-xs'
              }
              role={actionState === 'failed' ? 'alert' : 'status'}
            >
              {checkFailed
                ? m.dashboard_widget_operation_check_failed()
                : actionState === 'degraded'
                  ? m.dashboard_widget_proxy_mode_committed_degraded()
                  : actionState === 'unconfirmed'
                    ? m.dashboard_widget_proxy_mode_unconfirmed()
                    : m.dashboard_widget_proxy_mode_change_failed()}
            </p>
          )}
        </CardContent>
      </Card>
    </WidgetItem>
  );
}

export function ProxyModeWidget(props: WidgetComponentProps) {
  const { sourceOnly, isOverlay, disabled } = useDndGridContext();

  if (sourceOnly || isOverlay) return <ProxyModePreview id={props.id} />;

  return <ProxyModeLive {...props} disabled={!disabled} />;
}
