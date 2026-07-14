import { cn } from '@chimera/ui';
import type { ComponentProps } from 'react';

export default function WindowHeader({
  className,
  ...props
}: ComponentProps<'div'>) {
  return (
    <div
      className={cn(
        'dark:bg-primary-container bg-inverse-primary flex h-10 w-full',
        className,
      )}
      data-slot="app-header"
      data-tauri-drag-region
      {...props}
    />
  );
}
