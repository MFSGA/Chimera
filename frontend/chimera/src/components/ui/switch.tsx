import { cn } from '@chimera/ui';
import { Switch as SwitchPrimitives } from 'radix-ui';
import type { ComponentProps } from 'react';
import { CircularProgress } from './progress';

export const Switch = ({
  className,
  loading,
  ...props
}: ComponentProps<typeof SwitchPrimitives.Root> & { loading?: boolean }) => (
  <SwitchPrimitives.Root
    className={cn(
      'peer inline-flex h-8 w-14 shrink-0 cursor-pointer items-center rounded-full border-2 transition-colors',
      'focus-visible:ring-2 focus-visible:ring-offset-2 focus-visible:outline-none',
      'disabled:cursor-not-allowed disabled:opacity-50',
      'data-[state=checked]:border-transparent data-[state=unchecked]:border-transparent',
      'data-[state=checked]:bg-primary data-[state=unchecked]:bg-surface-variant',
      'dark:data-[state=checked]:bg-primary-container dark:data-[state=unchecked]:bg-on-surface-variant/30',
      className,
    )}
    {...props}
  >
    <SwitchPrimitives.Thumb
      className={cn(
        'group pointer-events-none block rounded-full shadow-lg ring-0 transition-all duration-200 ease-in-out',
        'data-[state=checked]:bg-surface data-[state=unchecked]:bg-on-surface/80',
        'dark:data-[state=checked]:bg-inverse-surface dark:data-[state=unchecked]:bg-inverse-surface',
        'data-[state=checked]:size-6 data-[state=unchecked]:size-4',
        'data-[state=checked]:translate-x-6.5 data-[state=unchecked]:translate-x-1.5',
      )}
    >
      {loading && (
        <span className="grid size-full place-items-center">
          <CircularProgress
            className="text-surface group-data-[state=checked]:size-4 group-data-[state=unchecked]:size-2.5"
            indeterminate
          />
        </span>
      )}
    </SwitchPrimitives.Thumb>
  </SwitchPrimitives.Root>
);

export function SwitchItem({
  children,
  className,
  ...props
}: ComponentProps<typeof Switch>) {
  return (
    <div
      className={cn(
        'bg-surface-variant/30 dark:bg-surface-variant/10 flex h-16 w-full items-center justify-between gap-2 rounded-xl p-4',
        className,
      )}
    >
      {children}
      <Switch {...props} />
    </div>
  );
}
