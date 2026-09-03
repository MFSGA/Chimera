import { openUWPTool, useClashConfig } from '@chimera/interface';
import { BaseCard, MenuItem, SwitchItem } from '@chimera/ui';
import { Button, List, ListItem, ListItemText } from '@mui/material';
import { useMemo } from 'react';
import { useTunStackModel } from '@/features/tun-stack/use-tun-stack';
import { useLockFn } from '@/hooks/use-lock-fn';
import { useCoreType } from '@/hooks/use-store';
import * as m from '@/paraglide/messages';
import { formatError } from '@/utils';
import getSystem from '@/utils/get-system';
import { message } from '@/utils/notification';

const isWIN = getSystem() === 'windows';

const AllowLan = () => {
  const { query, upsert } = useClashConfig();

  const value = useMemo(() => query.data?.['allow-lan'], [query.data]);

  const handleAllowLan = useLockFn(async (input: boolean) => {
    try {
      await upsert.mutateAsync({ 'allow-lan': input });
    } catch (error) {
      message(formatError(error), {
        title: m.common_error(),
        kind: 'error',
      });
    }
  });

  return (
    <SwitchItem
      label={m.settings_clash_settings_allow_lan_label()}
      checked={Boolean(value)}
      id="runtime-config-allow-lan"
      onChange={(_event, checked) => handleAllowLan(checked)}
    />
  );
};

const IPv6 = () => {
  const { query, upsert } = useClashConfig();

  const value = useMemo(() => query.data?.['ipv6'], [query.data]);
  const handleIPv6 = useLockFn(async (input: boolean) => {
    try {
      await upsert.mutateAsync({ ipv6: input });
    } catch (error) {
      message(formatError(error), {
        title: m.common_error(),
        kind: 'error',
      });
    }
  });

  return (
    <SwitchItem
      label={m.settings_clash_settings_ipv6_label()}
      checked={Boolean(value)}
      id="runtime-config-ipv6"
      onChange={(_event, checked) => handleIPv6(checked)}
    />
  );
};

const TunStack = () => {
  const [coreType] = useCoreType();

  const {
    execute: changeTunStack,
    options: tunStackOptions,
    selected,
    isPending,
  } = useTunStackModel(coreType);

  return (
    <MenuItem
      label={m.settings_clash_settings_tun_stack_label()}
      options={tunStackOptions}
      selected={selected}
      disabled={isPending}
      onSelected={(value) =>
        changeTunStack(value as keyof typeof tunStackOptions)
      }
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
  const handleLogLevel = useLockFn(async (input: string) => {
    try {
      await upsert.mutateAsync({ 'log-level': input });
    } catch (error) {
      message(formatError(error), {
        title: m.common_error(),
        kind: 'error',
      });
    }
  });

  return (
    <MenuItem
      id="runtime-config-log-level"
      label={m.settings_clash_settings_log_level_label()}
      options={options}
      selected={value ?? 'debug'}
      onSelected={(value) => handleLogLevel(value as string)}
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
