import { useSetting } from '@chimera/interface';
import { BaseCard } from '@chimera/ui';
import { Grid } from '@mui/material';
import { useTranslation } from 'react-i18next';
import { PaperSwitchButton } from './modules/system-proxy';

const AutoStartButton = () => {
  const { t } = useTranslation();
  const autoLaunch = useSetting('enable_auto_launch');

  return (
    <PaperSwitchButton
      label={t('Auto Start')}
      checked={autoLaunch.value || false}
      onClick={() => autoLaunch.upsert(!autoLaunch.value)}
    />
  );
};

const SilentStartButton = () => {
  const { t } = useTranslation();
  const silentStart = useSetting('enable_silent_start');

  return (
    <PaperSwitchButton
      label={t('Silent Start')}
      checked={silentStart.value || false}
      onClick={() => silentStart.upsert(!silentStart.value)}
    />
  );
};

export const SettingSystemBehavior = () => {
  const { t } = useTranslation();
  return (
    <BaseCard label={t('Initiating Behavior')}>
      <Grid container spacing={2}>
        <Grid size={{ xs: 6 }}>
          <AutoStartButton />
        </Grid>

        <Grid size={{ xs: 6 }}>
          <SilentStartButton />
        </Grid>
      </Grid>
    </BaseCard>
  );
};

export default SettingSystemBehavior;
