import { useSetting, useSystemService } from '@chimera/interface';
import { SwitchItem } from '@chimera/ui';
import { ListItem, Typography } from '@mui/material';
import { useLockFn } from 'ahooks';
import { useTranslation } from 'react-i18next';
import { formatError } from '@/utils';
import { message } from '@/utils/notification';

export const ServiceModeSwitch = () => {
  const { t } = useTranslation();
  const { query } = useSystemService();
  const serviceMode = useSetting('enable_service_mode');
  const isDisabled = query.data?.status === 'not_installed';

  const handleServiceMode = useLockFn(async () => {
    try {
      await serviceMode.upsert(!serviceMode.value);
    } catch (error) {
      message(
        `Activation Service Mode failed! \n Error: ${formatError(error)}`,
        {
          title: t('Error'),
          kind: 'error',
        },
      );
    }
  });

  return (
    <>
      <SwitchItem
        label={t('Service Mode')}
        disabled={isDisabled}
        checked={Boolean(serviceMode.value)}
        onChange={handleServiceMode}
      />

      {isDisabled && (
        <ListItem sx={{ pl: 0, pr: 0 }}>
          <Typography>
            {t(
              'Information: To enable service mode, make sure the Clash Nyanpasu service is installed and started',
            )}
          </Typography>
        </ListItem>
      )}
    </>
  );
};
