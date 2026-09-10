import { cn } from '@chimera/utils';
import type { ComponentProps } from 'react';
import * as m from '@/paraglide/messages';

export const HeaderSearch = ({
  className,
  ...props
}: ComponentProps<'input'>) => {
  return (
    <input
      autoComplete="off"
      spellCheck="false"
      placeholder={m.connections_search_placeholder()}
      className={cn(
        'bg-primary/10 h-10 min-w-0 rounded-full border-0 px-4 text-sm outline-none',
        'placeholder:text-on-surface-variant focus:ring-primary/40 focus:ring-2',
        className,
      )}
      {...props}
    />
  );
};

export default HeaderSearch;
