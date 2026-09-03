import {
  collectLogs,
  openAppConfigDir,
  openAppDataDir,
  openCoreDir,
  openLogsDir,
  setCustomAppDir,
} from '@chimera/interface';
import { BaseCard } from '@chimera/ui';
import Grid from '@mui/material/Grid';
import { open as openDialog } from '@tauri-apps/plugin-dialog';
import { useLockFn } from 'ahooks';
import * as m from '@/paraglide/messages';
import { formatError } from '@/utils';
import { message } from '@/utils/notification';
import { PaperButton } from './modules/paper-button';

const PathButton = ({
  label,
  onClick,
}: {
  label: string;
  onClick: () => Promise<unknown>;
}) => {
  const handleClick = useLockFn(async () => {
    try {
      await onClick();
    } catch (error) {
      await message(label + ': ' + formatError(error), {
        title: m.common_error(),
        kind: 'error',
      });
    }
  });

  return (
    <PaperButton
      label={label}
      onClick={handleClick}
      sxPaper={{ height: '100%' }}
      sxButton={{ height: '100%' }}
    />
  );
};

export const SettingChimeraPath = () => {
  const handleMigrateConfigDir = async () => {
    const selected = await openDialog({
      directory: true,
      multiple: false,
      title: m.settings_migrate_config_dir(),
    });

    if (!selected || Array.isArray(selected)) {
      return;
    }

    await setCustomAppDir(selected);
  };

  const buttonItems = [
    { label: m.settings_migrate_config_dir(), onClick: handleMigrateConfigDir },
    {
      label: m.settings_debug_utils_open_config_directory(),
      onClick: openAppConfigDir,
    },
    {
      label: m.settings_debug_utils_open_data_directory(),
      onClick: openAppDataDir,
    },
    {
      label: m.settings_debug_utils_open_core_directory(),
      onClick: openCoreDir,
    },
    {
      label: m.settings_debug_utils_open_log_directory(),
      onClick: openLogsDir,
    },
    {
      label: m.header_help_action_collect_logs(),
      onClick: collectLogs,
    },
  ];

  return (
    <BaseCard label={m.settings_path_config_title()}>
      <Grid container spacing={2} sx={{ alignItems: 'stretch' }}>
        {buttonItems.map(({ label, onClick }) => (
          <Grid
            key={label}
            size={{
              xs: 6,
              xl: 3,
            }}
          >
            <PathButton label={label} onClick={onClick} />
          </Grid>
        ))}
      </Grid>
    </BaseCard>
  );
};

export default SettingChimeraPath;
