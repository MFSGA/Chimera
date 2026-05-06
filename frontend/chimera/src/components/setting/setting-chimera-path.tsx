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
import { useTranslation } from 'react-i18next';
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
  const { t } = useTranslation();

  const handleClick = useLockFn(async () => {
    try {
      await onClick();
    } catch (error) {
      await message(`${label}: ${formatError(error)}`, {
        title: t('Error'),
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

const runCommand = async (
  command: () => Promise<
    { status: 'ok'; data: unknown } | { status: 'error'; error: string }
  >,
) => {
  const result = await command();
  if (result.status === 'error') {
    throw new Error(result.error);
  }
};

export const SettingChimeraPath = () => {
  const { t } = useTranslation();

  const handleMigrateConfigDir = async () => {
    const selected = await openDialog({
      directory: true,
      multiple: false,
      title: t('Migrate Config Dir'),
    });

    if (!selected || Array.isArray(selected)) {
      return;
    }

    await runCommand(() => setCustomAppDir(selected));
  };

  const buttonItems = [
    { label: t('Migrate Config Dir'), onClick: handleMigrateConfigDir },
    {
      label: t('Open Config Dir'),
      onClick: () => runCommand(openAppConfigDir),
    },
    { label: t('Open Data Dir'), onClick: () => runCommand(openAppDataDir) },
    { label: t('Open Core Dir'), onClick: () => runCommand(openCoreDir) },
    { label: t('Open Log Dir'), onClick: () => runCommand(openLogsDir) },
    { label: t('Collect Logs'), onClick: () => runCommand(collectLogs) },
  ];

  return (
    <BaseCard label={t('Path Config')}>
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
