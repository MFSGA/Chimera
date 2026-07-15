import { cn } from '@chimera/ui';
import { Link } from '@tanstack/react-router';
import ArrowBackIosNewRounded from '~icons/material-symbols/arrow-back-ios-new-rounded';
import type { ComponentProps } from 'react';
import { Button } from '@/components/ui/button';

export default function DetailHeader({
  type,
  children,
  className,
  ...props
}: ComponentProps<'div'> & { type: string }) {
  return (
    <div
      className={cn(
        'bg-mixed-background sticky top-0 z-10 flex h-16 items-center gap-4 px-4',
        className,
      )}
      {...props}
    >
      <Button icon className="flex items-center justify-center" asChild>
        <Link to="/main/profiles/$type" params={{ type }}>
          <ArrowBackIosNewRounded className="size-4" />
        </Link>
      </Button>

      {children}
    </div>
  );
}
