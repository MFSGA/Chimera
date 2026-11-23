import { cn } from '@chimera/ui';
import { Button, type ButtonProps } from '@chimera/ui/button';
import { CircularProgress } from '@chimera/ui/progress';
import { ShapeToggle } from '@chimera/ui/shape-toggle';
import NetworkPing from '~icons/material-symbols/network-ping-rounded';
import SettingsEthernet from '~icons/material-symbols/settings-ethernet-rounded';
import ShieldRounded from '~icons/material-symbols/shield-rounded';
import {
  useSystemProxyAction,
  useTransparentProxyAction,
  useTunModeAction,
} from '@/features/system-proxy/use-proxy-settings';
import * as m from '@/paraglide/messages';
import { formatError } from '@/utils';
import { message } from '@/utils/notification';

const reportProxyActionError = (error: unknown) => {
  void message(formatError(error), { title: m.common_error(), kind: 'error' });
};

export const ProxyButton = ({
  className,
  isActive,
  loading,
  disabled,
  children,
  ...props
}: ButtonProps & { isActive?: boolean }) => (
  <Button
    className={cn(
      'group h-14 font-semibold text-nowrap',
      'flex items-center justify-between gap-2',
      'data-[active=false]:bg-surface data-[active=false]:text-on-surface',
      'disabled:bg-surface disabled:text-on-surface/40 disabled:shadow-sm',
      className,
    )}
    data-active={String(Boolean(isActive))}
    data-loading={String(Boolean(loading))}
    variant="fab"
    {...props}
    disabled={Boolean(loading || disabled)}
  >
    <div className="flex items-center gap-3 [&_svg]:size-7">{children}</div>
    {loading && (
      <CircularProgress
        className={cn(
          'size-6 transition-opacity',
          'group-data-[loading=false]:opacity-0 group-data-[loading=true]:opacity-100',
        )}
        indeterminate
      />
    )}
  </Button>
);

type ShortcutButtonProps = Omit<ButtonProps, 'children' | 'loading'> & {
  presentation?: 'cookie';
  compact?: boolean;
  elastic?: boolean;
};

export const SystemProxyButton = ({
  presentation,
  compact,
  elastic,
  ...props
}: ShortcutButtonProps) => {
  const { execute, isPending, isActive, isReady } = useSystemProxyAction();
  const run = () => {
    void execute().catch(reportProxyActionError);
  };
  if (presentation === 'cookie') {
    return (
      <ShapeToggle
        {...props}
        compact={compact}
        active={isActive}
        loading={isPending}
        elastic={elastic}
        disabled={!isReady || isPending || props.disabled}
        title={!isReady ? m.common_loading() : props.title}
        onClick={run}
        icon={<NetworkPing />}
        label={m.settings_system_proxy_system_proxy_label()}
      />
    );
  }
  return (
    <ProxyButton
      {...props}
      loading={isPending}
      disabled={!isReady || isPending || props.disabled}
      title={!isReady ? m.common_loading() : props.title}
      onClick={run}
      isActive={isActive}
    >
      <NetworkPing />
      <span>{m.settings_system_proxy_system_proxy_label()}</span>
    </ProxyButton>
  );
};

export const TunModeButton = ({
  presentation,
  compact,
  elastic,
  ...props
}: ShortcutButtonProps) => {
  const { execute, isPending, isActive, blocked, blockedReason } =
    useTunModeAction();
  const run = () => {
    void execute().catch(reportProxyActionError);
  };
  if (presentation === 'cookie') {
    return (
      <ShapeToggle
        {...props}
        compact={compact}
        active={isActive}
        loading={isPending}
        elastic={elastic}
        disabled={blocked || isPending || props.disabled}
        title={blockedReason ?? props.title}
        onClick={run}
        icon={<SettingsEthernet />}
        label={m.settings_system_proxy_tun_mode_label()}
      />
    );
  }
  return (
    <ProxyButton
      {...props}
      disabled={blocked || isPending || props.disabled}
      title={blockedReason ?? props.title}
      loading={isPending}
      onClick={run}
      isActive={isActive}
    >
      <SettingsEthernet />
      <span>{m.settings_system_proxy_tun_mode_label()}</span>
    </ProxyButton>
  );
};

export const TransparentProxyButton = ({
  elastic,
  ...props
}: Omit<ShortcutButtonProps, 'presentation'>) => {
  const action = useTransparentProxyAction();
  if (!action.visible) return null;
  return (
    <ShapeToggle
      {...props}
      active={action.applied}
      data-configured={String(action.isActive)}
      data-applied={String(action.applied)}
      loading={action.isPending}
      disabled={action.disabled || props.disabled}
      title={action.reason}
      elastic={elastic}
      onClick={() => void action.execute()}
      icon={<ShieldRounded />}
      label={m.settings_clash_transparent_proxy_label()}
    />
  );
};
