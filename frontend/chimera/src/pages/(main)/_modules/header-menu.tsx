import { commands, openThat } from '@chimera/interface';
import { cn } from '@chimera/ui';
import { Link } from '@tanstack/react-router';
import type { ComponentProps, PropsWithChildren } from 'react';
import { Button, type ButtonProps } from '@/components/ui/button';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { useLockFn } from '@/hooks/use-lock-fn';
import * as m from '@/paraglide/messages';
import { formatEnvInfos } from '@/utils';
import { ProfileType } from '../main/profiles/_modules/consts';
import { Action } from '../main/profiles/$type';
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

const Menu = ({
  children,
  content,
}: PropsWithChildren<{ content: React.ReactNode }>) => (
  <DropdownMenu>
    <DropdownMenuTrigger asChild>{children}</DropdownMenuTrigger>
    <DropdownMenuContent align="start">{content}</DropdownMenuContent>
  </DropdownMenu>
);

const FileMenu = () => (
  <Menu
    content={
      <DropdownMenuItem asChild>
        <Link
          to="/main/profiles/$type"
          params={{ type: ProfileType.Profile }}
          search={{ action: Action.ImportLocalProfile }}
        >
          {m.header_file_action_import_local_profile()}
        </Link>
      </DropdownMenuItem>
    }
  >
    <MenuButton>{m.header_file_action_title()}</MenuButton>
  </Menu>
);

const HelpMenu = () => {
  const openIssues = useLockFn(async () => {
    const envs = await commands.collectEnvs();
    if (envs.status !== 'ok') return;

    const envInfos = encodeURIComponent(
      formatEnvInfos(envs.data)
        .split('\n')
        .map((line) => `> ${line}`)
        .join('\n'),
    );

    await openThat(
      'https://github.com/MFSGA/Chimera/issues/new?assignees=&labels=T%3A+Bug%2CS%3A+Untriaged&projects=&template=bug_report.yaml&env_infos=' +
        envInfos,
    );
  });

  const collectLogs = useLockFn(async () => {
    await commands.collectLogs();
  });

  return (
    <Menu
      content={
        <>
          <DropdownMenuItem
            onSelect={() => void openThat('https://github.com/MFSGA/Chimera')}
          >
            {m.header_help_action_github()}
          </DropdownMenuItem>
          <DropdownMenuItem onSelect={() => void openIssues()}>
            {m.header_help_action_issues()}
          </DropdownMenuItem>
          <DropdownMenuItem onSelect={() => void collectLogs()}>
            {m.header_help_action_collect_logs()}
          </DropdownMenuItem>
          <DropdownMenuSeparator />
          <DropdownMenuItem asChild>
            <Link to="/main/settings/about">
              {m.header_help_action_about()}
            </Link>
          </DropdownMenuItem>
        </>
      }
    >
      <MenuButton>{m.header_help_action_title()}</MenuButton>
    </Menu>
  );
};

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
      <FileMenu />
      <HeaderSettingsAction>
        <MenuButton>{m.header_settings_action_title()}</MenuButton>
      </HeaderSettingsAction>
      <HelpMenu />
    </div>
  );
}
