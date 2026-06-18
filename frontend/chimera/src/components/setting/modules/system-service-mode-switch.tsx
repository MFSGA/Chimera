import { useSetting, useSystemService } from '@chimera/interface';
import { SwitchItem } from '@chimera/ui';
import { ListItem, Typography } from '@mui/material';
import { useLockFn } from 'ahooks';
import * as m from '@/paraglide/messages';
import { formatError } from '@/utils';
import { message } from '@/utils/notification';

export const ServiceModeSwitch = () => {
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
          title: m.common_error(),
          kind: 'error',
        },
      );
    }
  });

  return (
    <>
      <SwitchItem
        label={m.settings_system_proxy_service_mode_label()}
        disabled={isDisabled}
        checked={Boolean(serviceMode.value)}
        onChange={handleServiceMode}
      />

      {isDisabled && (
        <ListItem sx={{ pl: 0, pr: 0 }}>
          <Typography>
            {m.settings_system_proxy_service_mode_description()}
          </Typography>
        </ListItem>
      )}
    </>
  );
};
