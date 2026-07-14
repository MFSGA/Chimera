import { cn } from '@chimera/ui';
import type { ComponentProps } from 'react';
import appIcon from '@root/backend/tauri/icons/icon.png';

export default function WindowTitle({
  children,
  className,
  ...props
}: ComponentProps<'div'>) {
  return (
    <div
      className={cn('flex items-center gap-2', className)}
      data-slot="app-header-logo-container"
      data-tauri-drag-region
      {...props}
    >
      <img className="size-5" src={appIcon} alt="" draggable={false} />
      {children}
    </div>
  );
}
