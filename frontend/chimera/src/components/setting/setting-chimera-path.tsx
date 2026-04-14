import {
  openAppConfigDir,
  openAppDataDir,
  openCoreDir,
  openLogsDir,
} from '@chimera/interface';
import { BaseCard } from '@chimera/ui';
import Grid from '@mui/material/Grid';
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

export const SettingChimeraPath = () => {
  const { t } = useTranslation();

  const buttonItems = [
    { label: t('Open Config Dir'), onClick: openAppConfigDir },
    { label: t('Open Data Dir'), onClick: openAppDataDir },
    { label: t('Open Core Dir'), onClick: openCoreDir },
    { label: t('Open Log Dir'), onClick: openLogsDir },
  ];

  return (
    <BaseCard label={t('Path Config')}>
      <Grid container alignItems="stretch" spacing={2}>
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
