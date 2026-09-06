import { useSetting } from '@chimera/interface';
import { alpha, cn } from '@chimera/ui';
import { SvgIconComponent } from '@mui/icons-material';
import { Box, ListItemButton, ListItemIcon, Tooltip } from '@mui/material';
import { useLocation, useNavigate } from '@tanstack/react-router';
import { createElement } from 'react';
import * as m from '@/paraglide/messages';
import { languageQuirks } from '@/utils/language';

const labelMap: Record<string, () => string> = {
  dashboard: m.navbar_label_dashboard,
  proxies: m.navbar_label_proxies,
  profiles: m.navbar_label_profiles,
  connections: m.navbar_label_connections,
  rules: m.navbar_label_rules,
  logs: m.navbar_label_logs,
  settings: m.navbar_label_settings,
  providers: m.navbar_label_providers,
};

export const RouteListItem = ({
  name,
  path,
  icon,
  onlyIcon,
}: {
  name: string;
  path: string;
  icon: SvgIconComponent;
  onlyIcon?: boolean;
}) => {
  const pathname = useLocation({
    select: (location) => location.pathname,
  });
  const isActive = pathname === path || pathname.startsWith(`${path}/`);

  const navigate = useNavigate();

  const { value: language } = useSetting('language');

  const listItemButton = (
    <ListItemButton
      data-testid={`sidebar-route-${name}`}
      className={cn(
        onlyIcon
          ? '!mx-auto !size-12 !rounded-2xl !p-0'
          : '!min-h-12 !rounded-2xl !px-4',
      )}
      sx={(theme) => ({
        backgroundColor: isActive
          ? alpha(theme.vars.palette.primary.main, 0.16)
          : 'transparent',
        transition: theme.transitions.create(['background-color', 'color'], {
          duration: theme.transitions.duration.shorter,
        }),
        '&:hover': {
          backgroundColor: alpha(
            theme.vars.palette.primary.main,
            isActive ? 0.24 : 0.08,
          ),
        },
      })}
      onClick={() => {
        navigate({
          to: path,
        });
      }}
    >
      <ListItemIcon
        sx={(theme) => ({
          minWidth: onlyIcon ? 0 : 40,
          justifyContent: 'center',
          color: isActive ? theme.vars.palette.primary.main : undefined,
        })}
      >
        {createElement(icon, {
          className: onlyIcon ? '!size-6' : undefined,
        })}
      </ListItemIcon>
      {!onlyIcon && (
        <Box
          className={cn(
            'w-full text-nowrap',
            language &&
              languageQuirks[language.toLowerCase()]?.drawer.itemClassNames,
          )}
          sx={(theme) => ({
            color: isActive ? theme.vars.palette.primary.main : undefined,
            fontWeight: isActive ? 600 : 400,
          })}
        >
          {labelMap[name]?.() ?? name}
        </Box>
      )}
    </ListItemButton>
  );

  return onlyIcon ? (
    <Tooltip title={labelMap[name]?.() ?? name}>{listItemButton}</Tooltip>
  ) : (
    listItemButton
  );
};

export default RouteListItem;
