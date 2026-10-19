import {
  useClashSetting,
  useSetting,
  type LoggingLevel_Serialize,
  type ProxiesSelectorMode,
  type ProxyChangeBreakMode,
} from '@chimera/interface';
import { BaseCard, MenuItem, SwitchItem } from '@chimera/ui';
import { List } from '@mui/material';
import { useLockFn } from 'ahooks';
import * as m from '@/paraglide/messages';
import { formatError } from '@/utils';
import { message } from '@/utils/notification';

const AppLogLevel = () => {
  const { value, upsert } = useSetting('app_log_level');

  const options = {
    trace: 'Trace',
    debug: 'Debug',
    info: 'Info',
    warn: 'Warn',
    error: 'Error',
    silent: 'Silent',
  };

  return (
    <MenuItem
      label={m.settings_chimera_app_log_level_label()}
      options={options}
      selected={value || 'info'}
      onSelected={(value) => upsert(value as LoggingLevel_Serialize)}
    />
  );
};

const BreakWhenProxyChange = () => {
  const breakWhenProxyChange = useClashSetting('break_connection');
  const checked = breakWhenProxyChange.value?.on_proxy_change !== 'off';

  const handleChange = useLockFn(async () => {
    try {
      await breakWhenProxyChange.upsert({
        on_proxy_change: checked ? 'off' : 'all',
      });
    } catch (error) {
      message(
        m.settings_proxies_break_change_update_failed() +
          '\n' +
          formatError(error),
        {
          title: m.common_error(),
          kind: 'error',
        },
      );
    }
  });

  return (
    <SwitchItem
      label={m.settings_chimera_enhance_break_when_proxy_change_label()}
      checked={checked}
      onChange={handleChange}
    />
  );
};

const BreakWhenProfileChange = () => {
  const breakWhenProfileChange = useClashSetting('break_connection');

  return (
    <SwitchItem
      label={m.settings_chimera_enhance_break_when_profile_change_label()}
      checked={Boolean(breakWhenProfileChange.value?.on_profile_change)}
      onChange={() =>
        breakWhenProfileChange.upsert({
          on_profile_change: !breakWhenProfileChange.value?.on_profile_change,
        })
      }
    />
  );
};

const BreakWhenModeChange = () => {
  const breakWhenModeChange = useClashSetting('break_connection');

  return (
    <SwitchItem
      label={m.settings_chimera_enhance_break_when_mode_change_label()}
      checked={Boolean(breakWhenModeChange.value?.on_mode_change)}
      onChange={() =>
        breakWhenModeChange.upsert({
          on_mode_change: !breakWhenModeChange.value?.on_mode_change,
        })
      }
    />
  );
};

const TrayProxiesSelector = () => {
  const { value, upsert } = useSetting('tray_selector_mode');

  const trayProxiesSelectorMode = {
    normal: m.settings_chimera_tray_type_normal(),
    hidden: m.settings_chimera_tray_type_hidden(),
    submenu: m.settings_chimera_tray_type_submenu(),
  };

  return (
    <MenuItem
      label={m.settings_chimera_proxies_selector_label()}
      options={trayProxiesSelectorMode}
      selected={value || 'normal'}
      onSelected={(value) => upsert(value as ProxiesSelectorMode)}
    />
  );
};

const EnableBuiltinEnhanced = () => {
  const { value, upsert } = useSetting('enable_builtin_enhanced');

  return (
    <SwitchItem
      label={m.settings_chimera_enhance_enable_builtin_enhanced_label()}
      checked={Boolean(value)}
      onChange={() => upsert(!value)}
    />
  );
};

const LightenAnimationEffects = () => {
  const { value, upsert } = useSetting('lighten_animation_effects');

  return (
    <SwitchItem
      id="chimera-lighten-animation-effects"
      label={m.settings_chimera_lighten_animations_label()}
      checked={Boolean(value)}
      onChange={() => upsert(!value)}
    />
  );
};

export const SettingChimeraMisc = () => {
  return (
    <BaseCard label={m.settings_label_chimera()}>
      <List disablePadding>
        <AppLogLevel />

        <TrayProxiesSelector />

        <BreakWhenProxyChange />

        <BreakWhenProfileChange />

        <BreakWhenModeChange />

        <EnableBuiltinEnhanced />

        <LightenAnimationEffects />
      </List>
    </BaseCard>
  );
};

export default SettingChimeraMisc;
