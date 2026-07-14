import { commands } from '@chimera/interface';
import { getSystem } from '@chimera/ui';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import type { ComponentProps } from 'react';
import {
  DefaultHeader,
  isMacOS,
  MacOSHeader,
  MacOSHeaderLeft,
} from '@/components/window/system-titlebar';
import WindowTitle from '@/components/window/window-title';
import HeaderMenu from './header-menu';

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

export default function Header({ className, ...props }: ComponentProps<'div'>) {
  return isMacOS ? (
    <MacOSHeader className={className} {...props}>
      <MacOSHeaderLeft>
        <HeaderMenu />
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
      <HeaderMenu className="hidden md:flex" />
    </DefaultHeader>
  );
}
