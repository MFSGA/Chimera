import { cn } from '@chimera/ui';
import { GitHub, HelpOutlined, Settings } from '@mui/icons-material';
import { IconButton, Tooltip } from '@mui/material';
import { Link } from '@tanstack/react-router';
import { ComponentProps } from 'react';

const APP_NAME = 'Clash Chimera';

export default function Header({ className, ...props }: ComponentProps<'div'>) {
  return (
    <div
      className={cn(
        'flex h-10 shrink-0 items-center justify-between px-3',
        'text-on-surface',
        className,
      )}
      data-slot="app-header"
      data-tauri-drag-region
      {...props}
    >
      <div
        className="text-base font-bold text-nowrap"
        data-slot="app-header-logo-name"
        data-tauri-drag-region
      >
        {APP_NAME}
      </div>

      <div className="flex items-center gap-1" data-slot="app-header-actions">
        <Tooltip title="GitHub">
          <IconButton
            size="small"
            color="inherit"
            onClick={() => {
              window.open('https://github.com/MFSGA/Chimera', '_blank');
            }}
          >
            <GitHub fontSize="small" />
          </IconButton>
        </Tooltip>

        <Tooltip title="Help">
          <IconButton size="small" color="inherit">
            <HelpOutlined fontSize="small" />
          </IconButton>
        </Tooltip>

        <Tooltip title="Settings">
          <IconButton size="small" color="inherit" component={Link} to="/main">
            <Settings fontSize="small" />
          </IconButton>
        </Tooltip>
      </div>
    </div>
  );
}
