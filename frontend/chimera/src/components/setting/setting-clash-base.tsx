import { openUWPTool } from '@chimera/interface';
import { BaseCard, MenuItem, SwitchItem } from '@chimera/ui';
import { Button, List, ListItem, ListItemText } from '@mui/material';
import {
  useClashBaseSettings,
  type ClashLogLevel,
} from '@/features/clash-settings/use-clash-base-settings';
import { useTunStackModel } from '@/features/tun-stack/use-tun-stack';
import { useCoreType } from '@/hooks/use-store';
import * as m from '@/paraglide/messages';
import getSystem from '@/utils/get-system';
import { message } from '@/utils/notification';

const isWIN = getSystem() === 'windows';

const AllowLan = () => {
  const { allowLan, isPending, setAllowLan } = useClashBaseSettings();

  return (
    <SwitchItem
      label={m.settings_clash_settings_allow_lan_label()}
      checked={allowLan}
      disabled={isPending}
      id="runtime-config-allow-lan"
      onChange={(_event, checked) => setAllowLan(checked)}
    />
  );
};

const IPv6 = () => {
  const { ipv6, isPending, setIPv6 } = useClashBaseSettings();

  return (
    <SwitchItem
      label={m.settings_clash_settings_ipv6_label()}
      checked={ipv6}
      disabled={isPending}
      id="runtime-config-ipv6"
      onChange={(_event, checked) => setIPv6(checked)}
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
  const { isPending, logLevel, logLevelOptions, setLogLevel } =
    useClashBaseSettings();

  return (
    <MenuItem
      id="runtime-config-log-level"
      label={m.settings_clash_settings_log_level_label()}
      options={logLevelOptions}
      selected={logLevel}
      disabled={isPending}
      onSelected={(value) => setLogLevel(value as ClashLogLevel)}
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
