import { cn } from '@chimera/utils';
import type { ComponentProps } from 'react';
import { Button, type ButtonProps } from '@/components/ui/button';
import * as m from '@/paraglide/messages';
import HeaderFileAction from './header-file-action';
import HeaderHelpAction from './header-help-action';
import HeaderSettingsAction from './header-settings-action';

const MenuButton = ({ className, ...props }: ButtonProps) => (
  <Button
    className={cn(
      'hover:bg-primary-container dark:hover:bg-on-primary h-8 min-w-0 px-3',
      'data-[state=open]:bg-primary-container dark:data-[state=open]:bg-on-primary',
      className,
    )}
    {...props}
  />
);

export default function HeaderMenu({
  className,
  ...props
}: ComponentProps<'div'>) {
  return (
    <div
      className={cn('flex items-center gap-0.5', className)}
      data-tauri-drag-region
      {...props}
    >
      <HeaderFileAction>
        <MenuButton>{m.header_file_action_title()}</MenuButton>
      </HeaderFileAction>

      <HeaderSettingsAction>
        <MenuButton>{m.header_settings_action_title()}</MenuButton>
      </HeaderSettingsAction>

      <HeaderHelpAction>
        <MenuButton>{m.header_help_action_title()}</MenuButton>
      </HeaderHelpAction>
    </div>
  );
}
