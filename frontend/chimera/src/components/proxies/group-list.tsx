import { useClashProxies, useProxyGroupIcons } from '@chimera/interface';
import { alpha } from '@chimera/ui';
import {
  ListItem,
  ListItemButton,
  ListItemButtonProps,
  ListItemIcon,
  ListItemText,
} from '@mui/material';
import { useAtom, useAtomValue } from 'jotai';
import { memo, RefObject, useDeferredValue, useMemo } from 'react';
import { Virtualizer } from 'virtua';
import { CacheImage } from '@/components/ui/image';
import { proxyGroupAtom } from '@/store';
import { proxiesFilterAtom } from '@/store/proxies';

const IconRender = memo(function IconRender({ icon }: { icon: string }) {
  return (
    <ListItemIcon>
      <CacheImage
        className="h-11 w-11"
        loadingClassName="rounded-full"
        icon={icon}
      />
    </ListItemIcon>
  );
});

export interface GroupListProps extends ListItemButtonProps {
  scrollRef: RefObject<HTMLElement>;
}

export const GroupList = ({
  scrollRef,
  ...listItemButtonProps
}: GroupListProps) => {
  const {
    proxies: { data },
  } = useClashProxies();
  const groupIcons = useProxyGroupIcons();

  const [proxyGroup, setProxyGroup] = useAtom(proxyGroupAtom);
  const proxiesFilter = useAtomValue(proxiesFilterAtom);
  const deferredProxiesFilter = useDeferredValue(proxiesFilter);

  const handleSelect = (index: number) => {
    setProxyGroup({ selector: index });
  };

  const groups = useMemo(() => {
    if (!data?.groups) {
      return [];
    }

    return data.groups
      .map((group, index) => ({ group, index }))
      .filter(({ group }) => {
        return (
          !deferredProxiesFilter ||
          group.name
            .toLowerCase()
            .includes(deferredProxiesFilter.toLowerCase()) ||
          group.all?.some((proxy) => {
            return proxy.name
              .toLowerCase()
              .includes(deferredProxiesFilter.toLowerCase());
          }) ||
          false
        );
      });
  }, [data?.groups, deferredProxiesFilter]);

  return (
    <Virtualizer scrollRef={scrollRef}>
      {groups.map(({ group, index }) => {
        const selected = index === proxyGroup.selector;

        return (
          <ListItem key={index} disablePadding>
            <ListItemButton
              selected={selected}
              onClick={() => handleSelect(index)}
              sx={[
                (theme) => ({
                  // note: beautify the css
                  borderRadius: 4,
                  margin: theme.spacing(0.5, 1),
                  padding: theme.spacing(1, 1.25),
                  backgroundColor: selected
                    ? `${alpha(theme.vars.palette.primary.main, 0.3)} !important`
                    : null,
                }),
              ]}
              {...listItemButtonProps}
            >
              {groupIcons[group.name] && (
                <IconRender icon={groupIcons[group.name]} />
              )}

              <ListItemText
                className="!truncate"
                primary={group.name}
                secondary={group.now}
              />
            </ListItemButton>
          </ListItem>
        );
      })}
    </Virtualizer>
  );
};
