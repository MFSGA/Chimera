import { cn, getSystem } from '@chimera/ui';
import type { ComponentProps } from 'react';
import useWindowMaximized from '@/hooks/use-window-maximized';
import WindowControl from './window-control';
import WindowHeader from './window-header';

const isMacOS = getSystem() === 'macos';

export function DefaultHeader({
  className,
  children,
  beforeClose,
  ...props
}: ComponentProps<'div'> & {
  beforeClose?: ComponentProps<typeof WindowControl>['beforeClose'];
}) {
  return (
    <WindowHeader
      className={cn('items-center justify-between px-3', className)}
      {...props}
    >
      <div className="flex items-center gap-2" data-tauri-drag-region>
        {children}
      </div>

      <WindowControl beforeClose={beforeClose} />
    </WindowHeader>
  );
}

export function MacOSHeader({ className, ...props }: ComponentProps<'div'>) {
  return (
    <WindowHeader
      className={cn('items-center justify-center px-3', className)}
      data-slot="app-header-macos"
      {...props}
    />
  );
}

export function MacOSHeaderLeft({
  className,
  ...props
}: ComponentProps<'div'>) {
  const { isMaximized } = useWindowMaximized();

  return (
    <div
      className={cn(
        'absolute hidden items-center md:flex',
        isMaximized ? 'left-2' : 'left-22',
        className,
      )}
      data-slot="app-header-macos-left"
      data-tauri-drag-region
      {...props}
    />
  );
}

export { isMacOS };
