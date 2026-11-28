import {
  nativeMemberDelay,
  useNativeProxies,
  useProxyGroupIcons,
  useProxyMode,
  type Proxies_Serialize,
  type ProxyGroup,
} from '@chimera/interface';
import { ScrollArea } from '@chimera/ui/scroll-area';
import TextMarquee from '@chimera/ui/text-marquee';
import { createFileRoute, Link } from '@tanstack/react-router';
import { useMemo } from 'react';
import DelayChip from '@/components/proxies/proxy-delay-chip';
import { CacheImage } from '@/components/ui/image';
import { m } from '@/paraglide/messages';
import { ActionButton } from '../_modules/action-button';
import BackButton from './_modules/back-button';

export const Route = createFileRoute('/(tray-menu)/tray-menu/proxies/')({
  component: RouteComponent,
});

const ProxyButton = ({
  proxy,
  proxies,
}: {
  proxy: ProxyGroup;
  proxies: Proxies_Serialize;
}) => {
  const icon = useProxyGroupIcons()[proxy.name];

  const currentDelay = useMemo(
    () =>
      proxy.now ? (nativeMemberDelay(proxy.now, proxy, proxies) ?? -1) : -1,
    [proxy, proxies],
  );

  return (
    <ActionButton disableClose asChild>
      <Link
        to="/tray-menu/proxies/group/$name"
        params={{
          name: proxy.name,
        }}
      >
        {proxy.iconConfigured && icon && (
          <CacheImage
            icon={icon}
            className="size-4.5"
            loadingClassName="rounded-full"
          />
        )}

        <TextMarquee className="min-w-0 flex-1">{proxy.name}</TextMarquee>

        {currentDelay > 0 && <DelayChip delay={currentDelay} />}
      </Link>
    </ActionButton>
  );
};

function RouteComponent() {
  const { data: proxies } = useNativeProxies();
  const { value: mode } = useProxyMode();
  const groups = proxies
    ? [
        ...(mode.global && proxies.global ? [proxies.global] : []),
        ...proxies.groups,
      ]
    : [];

  return (
    <ScrollArea className="h-dvh w-dvw">
      <div className="flex w-dvw flex-col gap-3 p-3">
        <BackButton>
          <span>{m.tray_menu_back_to_tray_menu()}</span>
        </BackButton>

        {proxies &&
          groups.map((group) => (
            <ProxyButton key={group.name} proxy={group} proxies={proxies} />
          ))}
      </div>
    </ScrollArea>
  );
}
