import { commands, openThat, useSetting } from '@chimera/interface';
import { Link } from '@tanstack/react-router';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import type { PropsWithChildren } from 'react';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/main-ui/dropdown-menu';
import { useLockFn } from '@/hooks/use-lock-fn';
import * as m from '@/paraglide/messages';
import { formatEnvInfos, formatError } from '@/utils';
import { message } from '@/utils/notification';

const currentWindow = getCurrentWebviewWindow();

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
