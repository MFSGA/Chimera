import { commands } from '@chimera/interface';
import { Link } from '@tanstack/react-router';
import type { PropsWithChildren } from 'react';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import {
  openBugReport,
  openProjectRepository,
} from '@/features/support/actions';
import { useUiSwitch } from '@/features/ui-switch/use-ui-switch';
import { useLockFn } from '@/hooks/use-lock-fn';
import * as m from '@/paraglide/messages';

const GitHubItem = () => {
  const handleClick = useLockFn(openProjectRepository);

  return (
    <DropdownMenuItem onClick={handleClick}>
      {m.header_help_action_github()}
    </DropdownMenuItem>
  );
};

const IssuesItem = () => {
  const handleClick = useLockFn(openBugReport);

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
