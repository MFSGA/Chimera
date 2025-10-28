import { alpha, cn } from '@chimera/ui';
import { SvgIconComponent } from '@mui/icons-material';
import { Box, ListItemButton, ListItemIcon, Tooltip } from '@mui/material';
import { useMatch, useNavigate } from '@tanstack/react-router';
import { createElement } from 'react';
import { useTranslation } from 'react-i18next';

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
  const { t } = useTranslation();

  const match = useMatch({
    strict: false,
    shouldThrow: false,
    from: path as never,
  });

  const navigate = useNavigate();

  const listItemButton = (
    <ListItemButton
      className={cn(
        onlyIcon ? '!mx-auto !size-16 !rounded-3xl' : '!rounded-full !pr-14',
      )}
      sx={[
        (theme) => ({
          backgroundColor: match ? alpha('red', 0.3) : alpha('orange', 0.15),
        }),
        (theme) => ({
          '&:hover': {
            backgroundColor: match ? alpha('red', 0.5) : null,
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
            fill: match ? 'yellow' : undefined,
          }),
          className: onlyIcon ? '!size-8' : undefined,
        })}
      </ListItemIcon>
      {!onlyIcon && (
        <Box
          className={cn('w-full text-nowrap pb-1 pt-1')}
          sx={(theme) => ({
            color: match ? 'red' : undefined,
          })}
        >
          {t(`label_${name}`)}
        </Box>
      )}
    </ListItemButton>
  );

  return onlyIcon ? (
    <Tooltip title={`label_${name}`}>{listItemButton}</Tooltip>
  ) : (
    listItemButton
  );
};

export default RouteListItem;
