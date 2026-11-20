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

const ProxyButton = ({
  className,
  isActive,
  loading,
  children,
  ...props
}: ButtonProps & { isActive?: boolean }) => (
  <Button
    className={cn(
      'group h-16 rounded-3xl font-bold text-nowrap',
      'flex items-center justify-between gap-2',
      'data-[active=false]:bg-white dark:data-[active=false]:bg-black',
      className,
    )}
    data-active={String(Boolean(isActive))}
    data-loading={String(Boolean(loading))}
    disabled={loading}
    variant="fab"
    {...props}
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
  elastic?: boolean;
};

export const SystemProxyButton = ({
  presentation,
  elastic,
  ...props
}: ShortcutButtonProps) => {
  const { execute, isPending, isActive } = useSystemProxyAction();
  if (presentation === 'cookie') {
    return (
      <ShapeToggle
        {...props}
        active={isActive}
        loading={isPending}
        elastic={elastic}
        onClick={() => void execute()}
        icon={<NetworkPing />}
        label={m.settings_system_proxy_system_proxy_label()}
      />
    );
  }
  return (
    <ProxyButton
      {...props}
      loading={isPending}
      onClick={() => void execute()}
      isActive={isActive}
    >
      <NetworkPing />
      <span>{m.settings_system_proxy_system_proxy_label()}</span>
    </ProxyButton>
  );
};

export const TunModeButton = ({
  presentation,
  elastic,
  ...props
}: ShortcutButtonProps) => {
  const { execute, isPending, isActive, blocked } = useTunModeAction();
  if (presentation === 'cookie') {
    return (
      <ShapeToggle
        {...props}
        active={isActive}
        loading={isPending}
        elastic={elastic}
        disabled={blocked || props.disabled}
        onClick={() => void execute()}
        icon={<SettingsEthernet />}
        label={m.settings_system_proxy_tun_mode_label()}
      />
    );
  }
  return (
    <ProxyButton
      {...props}
      disabled={blocked || props.disabled}
      loading={isPending}
      onClick={() => void execute()}
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
