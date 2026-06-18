import {
  openUWPTool,
  useClashConfig,
  useRuntimeProfile,
  useSetting,
  type TunStack as TunStackType,
} from '@chimera/interface';
import { BaseCard, MenuItem, SwitchItem } from '@chimera/ui';
import { Button, List, ListItem, ListItemText } from '@mui/material';
import { useMemo } from 'react';
import { useCoreType } from '@/hooks/use-store';
import * as m from '@/paraglide/messages';
import { formatError } from '@/utils';
import getSystem from '@/utils/get-system';
import { message } from '@/utils/notification';

const isWIN = getSystem() === 'windows';

const AllowLan = () => {
  const { query, upsert } = useClashConfig();

  const value = useMemo(() => query.data?.['allow-lan'], [query.data]);

  return (
    <SwitchItem
      label={m.settings_clash_settings_allow_lan_label()}
      checked={Boolean(value)}
      onChange={async () => {
        await upsert.mutateAsync({
          'allow-lan': !value,
        });
      }}
    />
  );
};

const IPv6 = () => {
  const { query, upsert } = useClashConfig();

  const value = useMemo(() => query.data?.['ipv6'], [query.data]);

  return (
    <SwitchItem
      label={m.settings_clash_settings_ipv6_label()}
      checked={Boolean(value)}
      onChange={async () => {
        await upsert.mutateAsync({
          ipv6: !value,
        });
      }}
    />
  );
};

const TunStack = () => {
  const [coreType] = useCoreType();

  const { value, upsert: upsertTunStack } = useSetting('tun_stack');

  const { value: enableTun, upsert: upsertTun } = useSetting('enable_tun_mode');

  const runtimeProfile = useRuntimeProfile();

  const tunStackOptions = useMemo(() => {
    const options: {
      [key: string]: string;
    } = {
      system: 'System',
      gvisor: 'gVisor',
      mixed: 'Mixed',
    };

    // clash not support mixed
    if (coreType === 'clash') {
      delete options.mixed;
    }
    return options;
  }, [coreType]);

  const selected = useMemo(() => {
    const stack = value || 'gvisor';
    return stack in tunStackOptions ? stack : 'gvisor';
  }, [tunStackOptions, value]);

  return (
    <MenuItem
      label={m.settings_clash_settings_tun_stack_label()}
      options={tunStackOptions}
      selected={selected}
      onSelected={async (value) => {
        try {
          await upsertTunStack(value as TunStackType);

          if (enableTun) {
            // just to reload clash config
            await upsertTun(true);
          }

          // need manual mutate to refetch runtime profile
          await runtimeProfile.refetch();
        } catch (error) {
          message(
            m.settings_clash_tun_stack_change_failed() +
              ' \n ' +
              formatError(error),
            {
              title: m.common_error(),
              kind: 'error',
            },
          );
        }
      }}
    />
  );
};

const LogLevel = () => {
  const { query, upsert } = useClashConfig();

  const options = {
    debug: 'Debug',
    info: 'Info',
    warning: 'Warn',
    error: 'Error',
    silent: 'Silent',
  };

  const value = useMemo(() => query.data?.['log-level'], [query.data]);

  return (
    <MenuItem
      label={m.settings_clash_settings_log_level_label()}
      options={options}
      selected={value ?? 'debug'}
      onSelected={async (value) => {
        await upsert.mutateAsync({
          'log-level': value as string,
        });
      }}
    />
  );
};

const UWPTool = () => {
  const handleClick = async () => {
    try {
      await openUWPTool();
    } catch (e) {
      message(
        m.settings_clash_uwp_tool_open_failed() + '\n' + JSON.stringify(e),
        {
          title: m.common_error(),
          kind: 'error',
        },
      );
    }
  };

  return (
    <ListItem sx={{ pl: 0, pr: 0 }}>
      <ListItemText primary={m.settings_clash_open_uwp_tool_label()} />

      <Button variant="contained" onClick={handleClick}>
        {m.common_open()}
      </Button>
    </ListItem>
  );
};

export const SettingClashBase = () => {
  const [coreType] = useCoreType();

  return (
    <BaseCard label={m.settings_clash_settings_title()}>
      <List disablePadding>
        <AllowLan />

        <IPv6 />

        {coreType !== 'clash-rs' && <TunStack />}

        <LogLevel />

        {isWIN && <UWPTool />}
      </List>
    </BaseCard>
  );
};

export default SettingClashBase;
