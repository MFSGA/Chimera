import { commands } from '@chimera/interface';
import { Link } from '@tanstack/react-router';
import type { PropsWithChildren } from 'react';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { useUiSwitch } from '@/features/ui-switch/use-ui-switch';
import { useLockFn } from '@/hooks/use-lock-fn';
import * as m from '@/paraglide/messages';
import { formatEnvInfos } from '@/utils';

const CHIMERA_REPOSITORY_URL = 'https://github.com/MFSGA/Chimera';

const GitHubItem = () => {
  const handleClick = useLockFn(async () => {
    await commands.openThat(CHIMERA_REPOSITORY_URL);
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

    const params = new URLSearchParams({
      assignees: '',
      labels: 'T%3A+Bug%2CS%3A+Untriaged',
      projects: '',
      template: 'bug_report.yaml',
    });

    await commands.openThat(
      `${CHIMERA_REPOSITORY_URL}/issues/new?${params.toString()}&env_infos=${formattedEnv}`,
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

const LegacyUiItem = () => {
  const { switchToLegacy, isPending } = useUiSwitch();

  return (
    <DropdownMenuItem
      disabled={isPending}
      onClick={() => void switchToLegacy()}
    >
      Switch to Legacy UI
    </DropdownMenuItem>
  );
};

export default function HeaderHelpAction({ children }: PropsWithChildren) {
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>{children}</DropdownMenuTrigger>

      <DropdownMenuContent>
        <DropdownMenuItem asChild>
          <Link to={'/main/assistant' as never}>{m.agent_title()}</Link>
        </DropdownMenuItem>

        <GitHubItem />

        <IssuesItem />

        <CollectLogItem />

        <LegacyUiItem />

        <DropdownMenuItem asChild>
          <Link to="/main/settings/about">{m.header_help_action_about()}</Link>
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
