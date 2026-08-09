import {
  collectLogs,
  openAppConfigDir,
  openAppDataDir,
  openCoreDir,
  openLogsDir,
  setCustomAppDir,
} from '@chimera/interface';
import { open as openDialog } from '@tauri-apps/plugin-dialog';
import { Button, type ButtonProps } from '@/components/ui/button';
import TextMarquee from '@/components/ui/text-marquee';
import { useLockFn } from '@/hooks/use-lock-fn';
import * as m from '@/paraglide/messages';
import { formatError } from '@/utils';
import { message } from '@/utils/notification';

const runCommand = async (
  command: () => Promise<
    { status: 'ok'; data: unknown } | { status: 'error'; error: string }
  >,
) => {
  const result = await command();
  if (result.status === 'error') throw new Error(result.error);
};

const PathButton = ({
  children,
  action,
  ...props
}: Omit<ButtonProps, 'onClick'> & {
  action: () => Promise<unknown> | unknown;
}) => {
  const handleClick = useLockFn(async () => {
    try {
      await action();
    } catch (error) {
      message(formatError(error), {
        title: m.common_error(),
        kind: 'error',
      });
    }
  });

  return (
    <Button
      {...props}
      variant="raised"
      className="h-18 w-full rounded-3xl px-5 text-left font-bold"
      onClick={() => void handleClick()}
    >
      <TextMarquee>{children}</TextMarquee>
    </Button>
  );
};

export default function PathUtilsCard() {
  const handleMigrateConfigDir = async () => {
    const selected = await openDialog({
      directory: true,
      multiple: false,
      title: m.settings_migrate_config_dir(),
    });
    if (!selected || Array.isArray(selected)) return;
    await runCommand(() => setCustomAppDir(selected));
  };

  const items = [
    {
      label: m.settings_debug_utils_open_config_directory(),
      action: () => runCommand(openAppConfigDir),
    },
    {
      label: m.settings_debug_utils_open_data_directory(),
      action: () => runCommand(openAppDataDir),
    },
    {
      label: m.settings_debug_utils_open_core_directory(),
      action: () => runCommand(openCoreDir),
    },
    {
      label: m.settings_debug_utils_open_log_directory(),
      action: () => runCommand(openLogsDir),
    },
    { label: m.settings_migrate_config_dir(), action: handleMigrateConfigDir },
    {
      label: m.header_help_action_collect_logs(),
      action: () => runCommand(collectLogs),
    },
  ];

  return (
    <div className="grid grid-cols-2 gap-2 md:grid-cols-3">
      {items.map(({ label, action }) => (
        <PathButton key={label} action={action}>
          {label}
        </PathButton>
      ))}
    </div>
  );
}
