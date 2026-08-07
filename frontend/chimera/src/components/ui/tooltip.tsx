import { cn } from '@chimera/ui';
import { motion } from 'motion/react';
import { Tooltip as TooltipPrimitive } from 'radix-ui';
import type { ComponentProps } from 'react';

export type TooltipProviderProps = ComponentProps<
  typeof TooltipPrimitive.Provider
>;

/** Provide zero-delay tooltips by default for desktop navigation controls. */
export function TooltipProvider({
  delayDuration = 0,
  ...props
}: TooltipProviderProps) {
  return <TooltipPrimitive.Provider delayDuration={delayDuration} {...props} />;
}

export type TooltipProps = ComponentProps<typeof TooltipPrimitive.Root>;

/** Render a tooltip root with a local zero-delay provider. */
export function Tooltip({ children, ...props }: TooltipProps) {
  return (
    <TooltipProvider>
      <TooltipPrimitive.Root {...props}>{children}</TooltipPrimitive.Root>
    </TooltipProvider>
  );
}

export type TooltipTriggerProps = ComponentProps<
  typeof TooltipPrimitive.Trigger
>;

export function TooltipTrigger(props: TooltipTriggerProps) {
  return <TooltipPrimitive.Trigger {...props} />;
}

export type TooltipContentProps = ComponentProps<
  typeof TooltipPrimitive.Content
> & {
  layout?: boolean | 'position' | 'size' | 'preserve-aspect';
};

/** Render the ref-style translucent rounded tooltip surface. */
export function TooltipContent({
  className,
  children,
  layout = 'preserve-aspect',
  sideOffset = 10,
  ...props
}: TooltipContentProps) {
  return (
    <TooltipPrimitive.Portal>
      <TooltipPrimitive.Content
        sideOffset={sideOffset}
        className={cn(
          'z-50 w-fit rounded-full text-xs text-balance',
          'dark:text-on-surface',
          'backdrop-blur-lg',
          'bg-primary-container/20 dark:bg-primary/10',
          'dark:shadow-inverse-on-surface/30 shadow-on-primary-container/30 shadow-sm',
          className,
        )}
        {...props}
      >
        <motion.div className="overflow-hidden px-3 py-1.5 text-xs text-balance">
          <motion.div layout={layout}>{children}</motion.div>
        </motion.div>
      </TooltipPrimitive.Content>
    </TooltipPrimitive.Portal>
  );
}
