import { SwitchItem } from '@chimera/ui';
import { ListItem, Typography } from '@mui/material';
import { useSystemServiceMode } from '@/features/system-service/use-system-service-mode';
import * as m from '@/paraglide/messages';

export const ServiceModeSwitch = () => {
  const serviceMode = useSystemServiceMode();
  const isDisabled = serviceMode.isNotInstalled;

  return (
    <>
      <SwitchItem
        label={m.settings_system_proxy_service_mode_label()}
        disabled={isDisabled}
        checked={serviceMode.value}
        onChange={serviceMode.toggle}
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
