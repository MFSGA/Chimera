import { useSetting } from '@chimera/interface';
import { alpha, cn } from '@chimera/ui';
import { SvgIconComponent } from '@mui/icons-material';
import { Box, ListItemButton, ListItemIcon, Tooltip } from '@mui/material';
import { useMatch, useNavigate } from '@tanstack/react-router';
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
  const match = useMatch({
    strict: false,
    shouldThrow: false,
    from: path as never,
  });

  const navigate = useNavigate();

  const { value: language } = useSetting('language');

  const listItemButton = (
    <ListItemButton
      className={cn(
        onlyIcon ? '!mx-auto !size-16 !rounded-3xl' : '!rounded-full !pr-14',
      )}
      sx={[
        (theme) => ({
          backgroundColor: match
            ? alpha(theme.vars.palette.primary.main, 0.3)
            : alpha(theme.vars.palette.background.paper, 0.15),
        }),
        (theme) => ({
          '&:hover': {
            backgroundColor: match
              ? alpha(theme.vars.palette.primary.main, 0.5)
              : null,
          },
        }),
      ]}
      onClick={() => {
        navigate({
          to: path,
        });
      }}
    >
      <ListItemIcon>
        {createElement(icon, {
          sx: (theme) => ({
            fill: match ? theme.vars.palette.primary.main : undefined,
          }),
          className: onlyIcon ? '!size-8' : undefined,
        })}
      </ListItemIcon>
      {!onlyIcon && (
        <Box
          className={cn(
            'w-full pt-1 pb-1 text-nowrap',
            language && languageQuirks[language].drawer.itemClassNames,
          )}
          sx={(theme) => ({
            color: match ? theme.vars.palette.primary.main : undefined,
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
