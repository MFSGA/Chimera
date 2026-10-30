import { cn } from '@chimera/ui';
import { Button } from '@chimera/ui/button';
import type { ComponentProps } from 'react';

export default function ActionButton({
  className,
  ...props
}: ComponentProps<typeof Button>) {
  return <Button className={cn('h-8 min-w-0 px-3', className)} {...props} />;
}
