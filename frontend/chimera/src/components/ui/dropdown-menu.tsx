import { cn } from '@chimera/ui';
import {
  ArrowRightRounded,
  CheckRounded,
  RadioButtonCheckedRounded,
} from '@mui/icons-material';
import { DropdownMenu as DropdownMenuPrimitive } from 'radix-ui';
import type { ComponentProps } from 'react';

const itemClassName = cn(
  'relative flex h-10 cursor-pointer items-center gap-2 rounded-lg px-3 text-sm outline-hidden',
  'data-[disabled]:pointer-events-none data-[disabled]:opacity-50',
  'focus:bg-surface-variant dark:focus:bg-surface-variant',
);

const contentClassName = cn(
  'bg-inverse-on-surface dark:bg-surface text-on-surface z-50 min-w-48 overflow-hidden rounded-xl border p-1 shadow-lg',
  'border-outline-variant/40 dark:border-outline-variant/20',
  'data-[state=open]:animate-in data-[state=closed]:animate-out',
  'data-[state=open]:fade-in-0 data-[state=closed]:fade-out-0',
  'data-[state=open]:zoom-in-95 data-[state=closed]:zoom-out-95',
);

export const DropdownMenu = DropdownMenuPrimitive.Root;
export const DropdownMenuTrigger = DropdownMenuPrimitive.Trigger;
export const DropdownMenuSub = DropdownMenuPrimitive.Sub;
export const DropdownMenuRadioGroup = DropdownMenuPrimitive.RadioGroup;

export function DropdownMenuContent({
  className,
  sideOffset = 6,
  align = 'center',
  ...props
}: ComponentProps<typeof DropdownMenuPrimitive.Content>) {
  return (
    <DropdownMenuPrimitive.Portal>
      <DropdownMenuPrimitive.Content
        className={cn(contentClassName, className)}
        sideOffset={sideOffset}
        align={align}
        {...props}
      />
    </DropdownMenuPrimitive.Portal>
  );
}

export function DropdownMenuSubContent({
  className,
  sideOffset = 6,
  ...props
}: ComponentProps<typeof DropdownMenuPrimitive.SubContent>) {
  return (
    <DropdownMenuPrimitive.Portal>
      <DropdownMenuPrimitive.SubContent
        className={cn(contentClassName, className)}
        sideOffset={sideOffset}
        {...props}
      />
    </DropdownMenuPrimitive.Portal>
  );
}

export function DropdownMenuItem({
  className,
  ...props
}: ComponentProps<typeof DropdownMenuPrimitive.Item>) {
  return (
    <DropdownMenuPrimitive.Item
      className={cn(itemClassName, className)}
      {...props}
    />
  );
}

export function DropdownMenuCheckboxItem({
  className,
  children,
  ...props
}: ComponentProps<typeof DropdownMenuPrimitive.CheckboxItem>) {
  return (
    <DropdownMenuPrimitive.CheckboxItem
      className={cn(itemClassName, 'pl-9', className)}
      {...props}
    >
      <DropdownMenuPrimitive.ItemIndicator className="absolute left-3 flex items-center">
        <CheckRounded fontSize="small" />
      </DropdownMenuPrimitive.ItemIndicator>
      {children}
    </DropdownMenuPrimitive.CheckboxItem>
  );
}

export function DropdownMenuRadioItem({
  className,
  children,
  ...props
}: ComponentProps<typeof DropdownMenuPrimitive.RadioItem>) {
  return (
    <DropdownMenuPrimitive.RadioItem
      className={cn(itemClassName, 'pr-9', className)}
      {...props}
    >
      {children}
      <DropdownMenuPrimitive.ItemIndicator className="absolute right-3 flex items-center">
        <RadioButtonCheckedRounded fontSize="small" />
      </DropdownMenuPrimitive.ItemIndicator>
    </DropdownMenuPrimitive.RadioItem>
  );
}

export function DropdownMenuSubTrigger({
  className,
  children,
  ...props
}: ComponentProps<typeof DropdownMenuPrimitive.SubTrigger>) {
  return (
    <DropdownMenuPrimitive.SubTrigger
      className={cn(itemClassName, 'justify-between', className)}
      {...props}
    >
      {children}
      <ArrowRightRounded fontSize="small" />
    </DropdownMenuPrimitive.SubTrigger>
  );
}

export function DropdownMenuSeparator({
  className,
  ...props
}: ComponentProps<typeof DropdownMenuPrimitive.Separator>) {
  return (
    <DropdownMenuPrimitive.Separator
      className={cn('bg-outline-variant/30 my-1 h-px', className)}
      {...props}
    />
  );
}
