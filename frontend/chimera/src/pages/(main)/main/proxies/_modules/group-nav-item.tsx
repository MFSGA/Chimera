import {
  type NativeProxies_Serialize,
  type ProxyGroup,
} from '@chimera/interface';
import {
  NavigationListItem,
  NavigationListItemIcon,
  NavigationListItemText,
  NavigationListItemTrailing,
  type NavigationListItemColor,
} from '@chimera/ui/navigation-list';
import { Link } from '@tanstack/react-router';
import AltRouteRounded from '~icons/material-symbols/alt-route-rounded';
import BalanceRounded from '~icons/material-symbols/balance-rounded';
import KeepRounded from '~icons/material-symbols/keep-rounded';
import RouteRounded from '~icons/material-symbols/route-rounded';
import ShieldRounded from '~icons/material-symbols/shield-rounded';
import SpeedRounded from '~icons/material-symbols/speed-rounded';
import TouchAppRounded from '~icons/material-symbols/touch-app-rounded';
import { memo, type ComponentType, type SVGProps } from 'react';
import { CacheImage } from '@/components/ui/image';
import DelayChip from './delay-chip';

type GroupGlyph = {
  color: NavigationListItemColor;
  icon: ComponentType<SVGProps<SVGSVGElement>>;
};

/** Keep the ref type icons even while group folders and DnD are not wired. */
const GLYPHS: Record<string, GroupGlyph> = {
  Selector: { color: 'blue', icon: TouchAppRounded },
  URLTest: { color: 'green', icon: SpeedRounded },
  Fallback: { color: 'orange', icon: ShieldRounded },
  LoadBalance: { color: 'purple', icon: BalanceRounded },
  Relay: { color: 'cyan', icon: RouteRounded },
};
const FALLBACK_GLYPH: GroupGlyph = {
  color: 'neutral',
  icon: AltRouteRounded,
};

const GroupNavItem = memo(function GroupNavItem({
  group,
  proxies,
  active,
}: {
  group: ProxyGroup;
  proxies: NativeProxies_Serialize;
  active: boolean;
}) {
  const glyph = GLYPHS[group.type] ?? FALLBACK_GLYPH;
  const selected = group.now ? proxies.nodes[group.now] : undefined;
  const history = selected?.history;
  const delay = history?.at(-1)?.delay;

  return (
    <NavigationListItem
      active={active}
      asChild
      data-slot="proxies-group-nav-item"
    >
      <Link
        to="/main/proxies/$name"
        params={{ name: group.name }}
        search={(previous) => ({ q: previous.q })}
        aria-current={active ? 'page' : undefined}
        data-group-name={group.name}
      >
        {group.icon ? (
          <NavigationListItemIcon image>
            <CacheImage
              icon={group.icon}
              className="size-full"
              loadingClassName="rounded-full"
            />
          </NavigationListItemIcon>
        ) : (
          <NavigationListItemIcon color={glyph.color}>
            <glyph.icon />
          </NavigationListItemIcon>
        )}

        <NavigationListItemText
          headline={group.name}
          supporting={group.now ?? ''}
          supportingStart={
            <>
              <span className="bg-secondary-container text-on-secondary-container shrink-0 rounded-full px-1.5 py-0.5 text-[10px] leading-3">
                {group.type}
              </span>
              {group.fixed && (
                <span
                  className="shrink-0"
                  data-slot="group-summary-fixed-icon"
                  title={group.fixed}
                >
                  <KeepRounded className="size-3.5" />
                </span>
              )}
            </>
          }
        />
        {delay !== undefined && (
          <NavigationListItemTrailing>
            {delay <= 0 ? (
              <span
                className="text-xs text-red-500"
                title="Latency test failed"
              >
                ∞
              </span>
            ) : (
              <DelayChip delay={delay} />
            )}
          </NavigationListItemTrailing>
        )}
      </Link>
    </NavigationListItem>
  );
});

export default GroupNavItem;
