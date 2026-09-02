import { cn } from '@chimera/ui';
import NetworkPing from '~icons/material-symbols/network-ping-rounded';
import SettingsEthernet from '~icons/material-symbols/settings-ethernet-rounded';
import { Button, type ButtonProps } from '@/components/ui/button';
import { CircularProgress } from '@/components/ui/progress';
import {
  useSystemProxyAction,
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

export const SystemProxyButton = (
  props: Omit<ButtonProps, 'children' | 'loading'>,
) => {
  const { execute, isPending, isActive } = useSystemProxyAction();

  return (
    <ProxyButton
      {...props}
      loading={isPending}
      onClick={execute}
      isActive={isActive}
    >
      <NetworkPing />
      <span>{m.settings_system_proxy_system_proxy_label()}</span>
    </ProxyButton>
  );
};

export const TunModeButton = (
  props: Omit<ButtonProps, 'children' | 'loading'>,
) => {
  const { execute, isPending, isActive } = useTunModeAction();

  return (
    <ProxyButton
      {...props}
      loading={isPending}
      onClick={execute}
      isActive={isActive}
    >
      <SettingsEthernet />
      <span>{m.settings_system_proxy_tun_mode_label()}</span>
    </ProxyButton>
  );
};
