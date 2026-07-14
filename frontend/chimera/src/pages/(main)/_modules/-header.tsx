import { commands } from '@chimera/interface';
import { cn, getSystem } from '@chimera/ui';
import { GitHub, HelpOutlined, Settings } from '@mui/icons-material';
import { IconButton, Tooltip } from '@mui/material';
import { Link } from '@tanstack/react-router';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import type { ComponentProps } from 'react';
import {
  DefaultHeader,
  isMacOS,
  MacOSHeader,
  MacOSHeaderLeft,
} from '@/components/window/system-titlebar';
import WindowTitle from '@/components/window/window-title';
import * as m from '@/paraglide/messages';

const APP_NAME = 'Clash Chimera';
const OS = getSystem();
const appWindow = getCurrentWebviewWindow();

const saveWindowStateBeforeClose = async () => {
  if (OS !== 'windows') {
    return true;
  }

  const result = await commands.saveWindowSizeState(appWindow.label);
  if (result.status === 'error') {
    console.error(result.error);
  }

  return true;
};

const Title = () => (
  <WindowTitle>
    <div
      className="text-on-surface text-base font-bold text-nowrap"
      data-slot="app-header-logo-name"
      data-tauri-drag-region
    >
      {APP_NAME}
    </div>
  </WindowTitle>
);

const HeaderActions = ({ className, ...props }: ComponentProps<'div'>) => (
  <div
    className={cn('flex items-center gap-1', className)}
    data-slot="app-header-actions"
    data-tauri-drag-region
    {...props}
  >
    <Tooltip title={m.header_help_action_github()}>
      <IconButton
        size="small"
        color="inherit"
        onClick={() =>
          window.open('https://github.com/MFSGA/Chimera', '_blank')
        }
      >
        <GitHub fontSize="small" />
      </IconButton>
    </Tooltip>

    <Tooltip title={m.header_help_action_title()}>
      <IconButton size="small" color="inherit">
        <HelpOutlined fontSize="small" />
      </IconButton>
    </Tooltip>

    <Tooltip title={m.header_settings_action_title()}>
      <IconButton
        size="small"
        color="inherit"
        component={Link}
        to="/main/settings"
      >
        <Settings fontSize="small" />
      </IconButton>
    </Tooltip>
  </div>
);

export default function Header({ className, ...props }: ComponentProps<'div'>) {
  return isMacOS ? (
    <MacOSHeader className={className} {...props}>
      <MacOSHeaderLeft>
        <HeaderActions />
      </MacOSHeaderLeft>
      <Title />
    </MacOSHeader>
  ) : (
    <DefaultHeader
      className={className}
      beforeClose={saveWindowStateBeforeClose}
      {...props}
    >
      <Title />
      <HeaderActions className="hidden md:flex" />
    </DefaultHeader>
  );
}
