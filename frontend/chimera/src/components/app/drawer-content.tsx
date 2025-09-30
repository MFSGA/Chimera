import { cn } from '@chimera/ui';
import { Box } from '@mui/material';
import getSystem from '@/utils/get-system';
import { getRoutesWithIcon } from '@/utils/routes-utils';
import RouteListItem from './modules/route-list-item';

export const DrawerContent = ({
  className,
  onlyIcon,
}: {
  className?: string;
  onlyIcon?: boolean;
}) => {
  const routes = getRoutesWithIcon();

  return (
    <Box
      className={cn(
        'p-4',
        getSystem() === 'macos' ? 'pt-14' : 'pt-8',
        'w-full',
        'h-full',
        'flex',
        'flex-col',
        'gap-4',
        className,
      )}
      sx={[
        {
          backgroundColor: 'var(--background-color-alpha)',
        },
      ]}
      data-tauri-drag-region
    >
      <div className="scrollbar-hidden flex flex-col gap-2 overflow-y-auto !overflow-x-hidden">
        {Object.entries(routes).map(([name, { path, icon }]) => {
          return (
            <RouteListItem
              key={name}
              name={name}
              path={path}
              icon={icon}
              onlyIcon={onlyIcon}
            />
          );
        })}
      </div>
    </Box>
  );
};

export default DrawerContent;
