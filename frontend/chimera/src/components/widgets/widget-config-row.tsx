import { cn } from '@chimera/utils';
import type { PropsWithChildren, ReactNode } from 'react';

export function ConfigGroup({ children }: PropsWithChildren) {
  return (
    <div
      className="border-outline-variant/30 flex flex-col gap-3 border-b px-4 py-3 last:border-b-0"
      data-slot="widget-config-group"
    >
      {children}
    </div>
  );
}

export function ConfigRow({
  label,
  htmlFor,
  className,
  children,
}: PropsWithChildren<{
  label: ReactNode;
  htmlFor?: string;
  className?: string;
}>) {
  const Label = htmlFor ? 'label' : 'span';

  return (
    <div
      className={cn('flex items-center justify-between gap-3', className)}
      data-slot="widget-config-row"
    >
      <Label htmlFor={htmlFor} className="min-w-0 text-sm">
        {label}
      </Label>
      {children}
    </div>
  );
}
