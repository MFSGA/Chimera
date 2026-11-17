import { useClashRules } from '@chimera/interface';
import {
  NavigationSidebar,
  NavigationSidebarContent,
  NavigationSidebarItem,
  NavigationSidebarToggle,
} from '@chimera/ui/navigation-sidebar';
import { ScrollArea } from '@chimera/ui/scroll-area';
import { SidebarProvider } from '@chimera/ui/slider-sidebar';
import { createFileRoute, Link, Outlet } from '@tanstack/react-router';
import ListRounded from '~icons/material-symbols/lists-rounded';
import { useMemo, type PropsWithChildren, type ReactNode } from 'react';
import z from 'zod';
import { keepReturn } from '@/components/router/cross-navigation';
import * as m from '@/paraglide/messages';
import { dataPageLayoutVariants } from '../_modules/data-page';
import ProxyIcon from './_modules/proxy-icon';

export const Route = createFileRoute('/(main)/main/rules')({
  component: RouteComponent,
  validateSearch: z.object({
    proxy: z.string().optional().nullable(),
    q: z.string().optional(),
  }),
});

const Item = ({
  item,
  children,
  icon,
}: PropsWithChildren<{
  item?: string;
  icon: ReactNode;
}>) => {
  const { proxy } = Route.useSearch();

  return (
    <NavigationSidebarItem
      active={(item ?? null) === (proxy ?? null)}
      icon={icon}
      data-slot="rules-proxy-item"
      data-proxy-item-kind={item === undefined ? 'all' : 'named'}
      data-proxy-item={item}
      renderLink={(content) => (
        <Link
          from={Route.fullPath}
          to="."
          search={(previous) => ({ ...previous, proxy: item })}
          state={keepReturn}
        >
          {content}
        </Link>
      )}
    >
      {children}
    </NavigationSidebarItem>
  );
};

const ProxySelector = () => {
  const { data } = useClashRules();

  const allProxy = useMemo(() => {
    const proxies =
      data?.rules
        .map((rule) => rule.proxy)
        .filter((proxy): proxy is string => !!proxy) ?? [];

    return [...new Set(proxies)];
  }, [data?.rules]);

  return (
    <NavigationSidebarContent className="pb-16">
      <Item icon={<ListRounded />}>{m.rules_list_all_proxies()}</Item>

      {allProxy.map((item) => (
        <Item key={item} item={item} icon={<ProxyIcon groupName={item} />}>
          {item}
        </Item>
      ))}
    </NavigationSidebarContent>
  );
};

function RouteComponent() {
  return (
    <SidebarProvider defaultOpen={false}>
      <div className={dataPageLayoutVariants()} data-slot="rules-layout">
        <NavigationSidebar aria-label={m.navbar_label_rules()}>
          <ScrollArea className="min-h-0 w-full flex-1 [&>div>div]:block!">
            <ProxySelector />
          </ScrollArea>
          <NavigationSidebarToggle expandLabel={m.navigation_sidebar_expand()}>
            {m.navigation_sidebar_collapse()}
          </NavigationSidebarToggle>
        </NavigationSidebar>
        <Outlet />
      </div>
    </SidebarProvider>
  );
}
