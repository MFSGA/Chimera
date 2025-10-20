import { BaseCard } from '@chimera/ui';
import { Grid } from '@mui/material';
import { useTranslation } from 'react-i18next';

const TunModeButton = () => {
  const { t } = useTranslation();

  return <div>TunModeButton</div>;
};

const SystemProxyButton = () => {
  return <div>SystemProxyButton</div>;
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
