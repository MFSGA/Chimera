import { commands, useSetting } from '@chimera/interface';
import { Link } from '@tanstack/react-router';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
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
import { useLockFn } from '@/hooks/use-lock-fn';
import * as m from '@/paraglide/messages';
import { formatError } from '@/utils';
import { message } from '@/utils/notification';

const currentWindow = getCurrentWebviewWindow();

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
  const windowType = useSetting('window_type');

  const handleClick = useLockFn(async () => {
    try {
      await windowType.upsert('legacy');
      const result = await commands.createLegacyWindow();

      if (result.status !== 'ok') {
        throw new Error(result.error);
      }

      await currentWindow.close();
    } catch (error) {
      await message(`Failed to open legacy UI: ${formatError(error)}`, {
        kind: 'error',
        title: m.common_error(),
      });
    }
  });

  return (
    <DropdownMenuItem
      disabled={windowType.isPending}
      onClick={() => void handleClick()}
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
