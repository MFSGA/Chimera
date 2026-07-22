import { commands, openThat } from '@chimera/interface';
import { Link } from '@tanstack/react-router';
import type { PropsWithChildren } from 'react';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { useLockFn } from '@/hooks/use-lock-fn';
import * as m from '@/paraglide/messages';
import { formatEnvInfos } from '@/utils';

const GitHubItem = () => {
  const handleClick = useLockFn(async () => {
    await openThat('https://github.com/MFSGA/Chimera');
  });

  return (
    <DropdownMenuItem onClick={handleClick}>
      {m.header_help_action_github()}
    </DropdownMenuItem>
  );
};

const IssuesItem = () => {
  const handleClick = useLockFn(async () => {
    const envs = await commands.collectEnvs();

    if (envs.status !== 'ok') {
      return;
    }

    const formattedEnv = encodeURIComponent(
      formatEnvInfos(envs.data)
        .split('\n')
        .map((value) => `> ${value}`)
        .join('\n'),
    );

    await openThat(
      'https://github.com/MFSGA/Chimera/issues/new?assignees=&labels=T%3A+Bug%2CS%3A+Untriaged&projects=&template=bug_report.yaml&env_infos=' +
        formattedEnv,
    );
  });

  return (
    <DropdownMenuItem onClick={handleClick}>
      {m.header_help_action_issues()}
    </DropdownMenuItem>
  );
};

const CollectLogItem = () => {
  const handleClick = useLockFn(async () => {
    await commands.collectLogs();
  });

  return (
    <DropdownMenuItem onClick={handleClick}>
      {m.header_help_action_collect_logs()}
    </DropdownMenuItem>
  );
};

export default function HeaderHelpAction({ children }: PropsWithChildren) {
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>{children}</DropdownMenuTrigger>

      <DropdownMenuContent>
        <GitHubItem />
        <IssuesItem />
        <CollectLogItem />

        <DropdownMenuItem asChild>
          <Link to="/main/settings/about">{m.header_help_action_about()}</Link>
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
