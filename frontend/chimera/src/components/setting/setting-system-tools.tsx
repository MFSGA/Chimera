import { flushSystemDnsCache } from '@chimera/interface';
import { BaseCard } from '@chimera/ui';
import { Button, List, ListItem, ListItemText } from '@mui/material';
import { useLockFn } from 'ahooks';
import * as m from '@/paraglide/messages';
import { formatError } from '@/utils';
import getSystem from '@/utils/get-system';
import { message } from '@/utils/notification';

const system = getSystem();
const isSupported = system === 'windows' || system === 'macos';

export const SettingSystemTools = () => {
  const handleFlushDnsCache = useLockFn(async () => {
    try {
      await flushSystemDnsCache();
      await message(m.settings_system_proxy_dns_cache_success(), {
        kind: 'info',
      });
    } catch (error) {
      await message(
        `${m.settings_system_proxy_dns_cache_failed()}: ${formatError(error)}`,
        { kind: 'error' },
      );
    }
  });

  if (!isSupported) {
    return null;
  }

  return (
    <BaseCard label={m.settings_system_proxy_system_tools_label()}>
      <List disablePadding>
        <ListItem sx={{ pl: 0, pr: 0 }}>
          <ListItemText
            primary={m.settings_system_proxy_dns_cache_label()}
            secondary={m.settings_system_proxy_dns_cache_description()}
          />

          <Button variant="contained" onClick={handleFlushDnsCache}>
            {m.settings_system_proxy_dns_cache_label()}
          </Button>
        </ListItem>
      </List>
    </BaseCard>
  );
};

export default SettingSystemTools;
