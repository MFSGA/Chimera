import { Tooltip, TooltipContent, TooltipTrigger } from '@chimera/ui/tooltip';
import type { ReactElement } from 'react';

/** A card action whose label shows in the profiles pages' tooltip. */
export default function ProfileCardAction({
  label,
  children,
}: {
  label: string;
  children: ReactElement;
}) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>{children}</TooltipTrigger>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
  );
}
