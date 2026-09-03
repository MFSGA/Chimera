import type { ComponentProps } from 'react';
import {
  DefaultHeader,
  MacOSHeader,
  MacOSHeaderLeft,
} from '@/components/window/system-titlebar';
import WindowTitle from '@/components/window/window-title';
import { isMacOS } from '@/consts';
import { saveCurrentWindowState } from '@/features/window/actions';
import HeaderMenu from './header-menu';

const APP_NAME = 'Clash Chimera';
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
      beforeClose={saveCurrentWindowState}
      {...props}
    >
      <Title />
      <HeaderMenu className="hidden md:flex" />
    </DefaultHeader>
  );
}
