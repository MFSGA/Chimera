import {
  nativeMemberDelay,
  useNativeProxies,
  useNativeProxyActions,
  useProxyMode,
  type Proxy_Serialize,
  type ProxyGroup,
} from '@chimera/interface';
import { useScrollAreaViewportElement } from '@chimera/ui/scroll-area';
import TextMarquee from '@chimera/ui/text-marquee';
import { cn } from '@chimera/utils';
import { createFileRoute } from '@tanstack/react-router';
import { useVirtualizer } from '@tanstack/react-virtual';
import BoltRounded from '~icons/material-symbols/bolt-rounded';
import KeepOffRounded from '~icons/material-symbols/keep-off-rounded';
import KeepRounded from '~icons/material-symbols/keep-rounded';
import { useMemo } from 'react';
import { useBlockTask } from '@/components/providers/block-task-provider';
import DelayChip from '@/components/proxies/proxy-delay-chip';
import { useLockFn } from '@/hooks/use-lock-fn';
import { m } from '@/paraglide/messages';
import BackButton from '../_modules/back-button';
import { ActionButton } from '../../_modules/action-button';

export const Route = createFileRoute(
  '/(tray-menu)/tray-menu/proxies/group/$name',
)({
  component: RouteComponent,
  staticData: { crumb: { title: ({ params }) => params.name } },
});

const DelayTestButton = () => {
  const { name } = Route.useParams();

  const { testGroupDelay } = useNativeProxyActions();

  const blockTask = useBlockTask(`tray-delay-group-test-${name}`, async () => {
    await testGroupDelay(name);
  });

  return (
    <ActionButton
      className="w-10 shrink-0 justify-center backdrop-blur-lg"
      disableClose
      onClick={() => blockTask.execute()}
      loading={blockTask.isPending}
    >
      <BoltRounded />
    </ActionButton>
  );
};

const ClearFixedButton = ({ group }: { group: string }) => {
  const { clearProxyFixed } = useNativeProxyActions();

  const handleClick = async () => {
    try {
      await clearProxyFixed(group);
    } catch (error) {
      // A dialog would take focus and dismiss the tray menu; the frontend
      // error reporter records this in the application log.
      console.error('[tray-menu] failed to restore automatic selection', error);
    }
  };

  return (
    <ActionButton
      className="w-10 shrink-0 justify-center backdrop-blur-lg"
      title={m.proxies_group_clear_fixed_button()}
      disableClose
      onClick={handleClick}
    >
      <KeepOffRounded />
    </ActionButton>
  );
};

const ProxyButton = ({
  proxy,
  selectable,
  fixed,
  delay,
  onSelect,
}: {
  proxy: Proxy_Serialize;
  selectable: boolean;
  fixed: boolean;
  delay: number;
  onSelect: (proxy: Proxy_Serialize) => Promise<void>;
}) => {
  const handleClick = useLockFn(async () => {
    await onSelect(proxy);
  });

  return (
    <ActionButton
      className="w-full data-[selectable=false]:cursor-default"
      data-selectable={String(selectable)}
      // The core picks this group's member on its own.
      disabled={!selectable}
      onClick={handleClick}
    >
      <TextMarquee className="min-w-0 flex-1">{proxy.name}</TextMarquee>

      {fixed && (
        <span
          className="text-primary shrink-0"
          title={m.proxies_group_fixed_label()}
          data-slot="tray-menu-proxy-fixed-icon"
        >
          <KeepRounded className="size-4" />
        </span>
      )}

      {delay > 0 && <DelayChip delay={delay} />}
    </ActionButton>
  );
};

function RouteComponent() {
  const { name: proxyGroupName } = Route.useParams();

  const { data: proxies } = useNativeProxies();
  const { selectProxy } = useNativeProxyActions();

  const { value: proxyMode } = useProxyMode();

  const currentGroup = useMemo<ProxyGroup | undefined>(() => {
    if (proxyMode.global && proxies?.global?.name === proxyGroupName) {
      return proxies.global;
    }

    return proxies?.groups.find((group) => group.name === proxyGroupName);
  }, [proxies, proxyGroupName, proxyMode]);

  const handleSelectProxy = async (proxy: Proxy_Serialize) => {
    if (!currentGroup) {
      return;
    }

    try {
      await selectProxy(currentGroup.name, proxy.name);
    } catch (error) {
      // A dialog would take focus and dismiss the tray menu; the frontend
      // error reporter records this in the application log.
      console.error('[tray-menu] failed to select proxy', error);
    }
  };

  const viewportElement = useScrollAreaViewportElement();

  const virtualizer = useVirtualizer({
    count: currentGroup?.all?.length || 0,
    getScrollElement: () => viewportElement,
    estimateSize: () => 60,
    overscan: 5,
    measureElement: (element) => element?.getBoundingClientRect().height,
  });

  const virtualItems = virtualizer.getVirtualItems();

  return (
    <div className="w-dvw p-3">
      <div className="sticky top-3 z-10 flex gap-2">
        <BackButton className="block">
          <span>{m.tray_menu_back_to_proxies_menu()}</span>
        </BackButton>

        {currentGroup?.fixed && currentGroup.capabilities.clearFixed && (
          <ClearFixedButton group={currentGroup.name} />
        )}

        <DelayTestButton />
      </div>

      <div
        className="relative"
        data-slot="proxies-virtual-list"
        style={{
          height: `${virtualizer.getTotalSize()}px`,
        }}
      >
        {virtualItems.map((virtualItem) => {
          const name = currentGroup?.all?.[virtualItem.index];
          const proxy = name ? proxies?.nodes[name] : undefined;

          if (!proxy) {
            return null;
          }

          return (
            <div
              key={virtualItem.index}
              ref={virtualizer.measureElement}
              className={cn(
                'absolute top-0 left-0 w-full',
                'flex flex-col pt-3',
              )}
              style={{
                transform: `translateY(${virtualItem.start}px)`,
              }}
              data-index={virtualItem.index}
              data-slot="proxies-virtual-item"
              data-active={String(name === currentGroup?.now)}
            >
              <ProxyButton
                proxy={proxy}
                selectable={currentGroup?.capabilities.select ?? false}
                fixed={name === currentGroup?.fixed}
                delay={
                  name && currentGroup && proxies
                    ? (nativeMemberDelay(name, currentGroup, proxies) ?? -1)
                    : -1
                }
                onSelect={handleSelectProxy}
              />
            </div>
          );
        })}
      </div>
    </div>
  );
}
