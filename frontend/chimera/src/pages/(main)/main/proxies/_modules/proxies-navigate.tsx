import { useNativeProxies } from '@chimera/interface';
import {
  NavigationList,
  NavigationListGroup,
} from '@chimera/ui/navigation-list';
import { useParams } from '@tanstack/react-router';
import * as m from '@/paraglide/messages';
import GroupNavItem from './group-nav-item';

/** Ref NavigationList presentation over Chimera's existing native proxy graph. */
export default function ProxiesNavigate() {
  const { data: proxies } = useNativeProxies();
  const { name: activeName } = useParams({ strict: false });

  if (!proxies) return null;

  return (
    <NavigationList
      aria-label={m.proxies_nav_label()}
      data-slot="proxies-navigate"
    >
      <NavigationListGroup label={m.proxies_nav_groups()}>
        {proxies.groups.map((group) => (
          <GroupNavItem
            key={group.name}
            group={group}
            proxies={proxies}
            active={group.name === activeName}
          />
        ))}
      </NavigationListGroup>
    </NavigationList>
  );
}
