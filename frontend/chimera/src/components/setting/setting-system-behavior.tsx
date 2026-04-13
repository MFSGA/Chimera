import { useSetting } from '@chimera/interface';
import { BaseCard } from '@chimera/ui';
import Grid from '@mui/material/Grid';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import { useTranslation } from 'react-i18next';
import { PaperSwitchButton } from './modules/system-proxy';

const appWindow = getCurrentWebviewWindow();

const AlwaysOnTopButton = () => {
  const { t } = useTranslation();
  const alwaysOnTop = useSetting('always_on_top');

  const handleToggle = async () => {
    const nextValue = !alwaysOnTop.value;
    await alwaysOnTop.upsert(nextValue);
    await appWindow.setAlwaysOnTop(nextValue);
  };

  return (
    <PaperSwitchButton
      label={t('Always On Top')}
      checked={Boolean(alwaysOnTop.value)}
      onClick={handleToggle}
    />
  );
};

const AutoCheckUpdateButton = () => {
  const { t } = useTranslation();
  const autoCheckUpdate = useSetting('enable_auto_check_update');

  return (
    <PaperSwitchButton
      label={t('Auto Check Updates')}
      checked={autoCheckUpdate.value ?? true}
      onClick={() => autoCheckUpdate.upsert(!(autoCheckUpdate.value ?? true))}
    />
  );
};

export const SettingSystemBehavior = () => {
  const { t } = useTranslation();

  return (
    <BaseCard label={t('Initiating Behavior')}>
      <Grid container spacing={2}>
        <Grid size={{ xs: 6 }}>
          <AlwaysOnTopButton />
        </Grid>

        <Grid size={{ xs: 6 }}>
          <AutoCheckUpdateButton />
        </Grid>
      </Grid>
    </BaseCard>
  );
};

export default SettingSystemBehavior;
