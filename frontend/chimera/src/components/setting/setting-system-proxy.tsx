import { useSetting } from '@chimera/interface';
import { BaseCard } from '@chimera/ui';
import { Grid } from '@mui/material';
import { useLockFn } from 'ahooks';
import { useTranslation } from 'react-i18next';
import { formatError } from '@/utils';
import { message } from '@/utils/notification';
import { PaperSwitchButton } from './modules/system-proxy';

const TunModeButton = () => {
  const { t } = useTranslation();

  const tunMode = useSetting('enable_tun_mode');

  const handleTunMode = useLockFn(async () => {
    try {
      await tunMode.upsert(!tunMode.value);
    } catch (error) {
      message(`Activation TUN Mode failed! \n Error: ${formatError(error)}`, {
        title: t('Error'),
        kind: 'error',
      });
    }
  });

  return (
    <PaperSwitchButton
      label={t('TUN Mode')}
      checked={Boolean(tunMode.value)}
      onClick={handleTunMode}
    />
  );
};

const SystemProxyButton = () => {
  const { t } = useTranslation();

  const systemProxy = useSetting('enable_system_proxy');

  const handleSystemProxy = useLockFn(async () => {
    try {
      await systemProxy.upsert(!systemProxy.value);
    } catch (error) {
      message(`Activation System Proxy failed!`, {
        title: t('Error'),
        kind: 'error',
      });
    }
  });

  return (
    <PaperSwitchButton
      label={t('System Proxy')}
      checked={Boolean(systemProxy.value)}
      onClick={handleSystemProxy}
    />
  );
};

export const SettingSystemProxy = () => {
  const { t } = useTranslation();

  return (
    <BaseCard
      label={t('System Setting')}
      /* labelChildren={
        <ExpandMore expand={expand} onClick={() => setExpand(!expand)} />
      } */
    >
      <Grid container spacing={2}>
        <Grid size={{ xs: 6 }}>
          <TunModeButton />
        </Grid>

        <Grid size={{ xs: 6 }}>
          <SystemProxyButton />
        </Grid>
      </Grid>
    </BaseCard>
  );
};

export default SettingSystemProxy;
