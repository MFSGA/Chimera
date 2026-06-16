import { useSetting } from '@chimera/interface';
import { BaseCard } from '@chimera/ui';
import { Grid } from '@mui/material';
import * as m from '@/paraglide/messages';
import { PaperSwitchButton } from './modules/system-proxy';

const AutoStartButton = () => {
  const autoLaunch = useSetting('enable_auto_launch');

  return (
    <PaperSwitchButton
      label={m.settings_system_proxy_auto_launch_label()}
      checked={autoLaunch.value || false}
      onClick={() => autoLaunch.upsert(!autoLaunch.value)}
    />
  );
};

const SilentStartButton = () => {
  const silentStart = useSetting('enable_silent_start');

  return (
    <PaperSwitchButton
      label={m.settings_system_proxy_silent_start_label()}
      checked={silentStart.value || false}
      onClick={() => silentStart.upsert(!silentStart.value)}
    />
  );
};

export const SettingSystemBehavior = () => {
  return (
    <BaseCard label={'Initiating Behavior'}>
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
