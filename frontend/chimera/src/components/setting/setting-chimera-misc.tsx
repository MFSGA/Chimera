import {
  useSetting,
  type BreakWhenProxyChange as BreakWhenProxyChangeType,
  type LoggingLevel,
  type ProxiesSelectorMode,
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
      label={m.settings_nyanpasu_app_log_level_label()}
      options={options}
      selected={value || 'info'}
      onSelected={(value) => upsert(value as LoggingLevel)}
    />
  );
};

const BreakWhenProxyChange = () => {
  const breakWhenProxyChange = useSetting('break_when_proxy_change');
  const checked = breakWhenProxyChange.value
    ? breakWhenProxyChange.value !== 'none'
    : false;

  const handleChange = useLockFn(async () => {
    try {
      await breakWhenProxyChange.upsert(
        (checked ? 'none' : 'all') as BreakWhenProxyChangeType,
      );
    } catch (error) {
      message(`Update break when proxy change failed!\n${formatError(error)}`, {
        title: 'Error',
        kind: 'error',
      });
    }
  });

  return (
    <SwitchItem
      label={m.settings_nyanpasu_enhance_break_when_proxy_change_label()}
      checked={checked}
      onChange={handleChange}
    />
  );
};

const BreakWhenProfileChange = () => {
  const breakWhenProfileChange = useSetting('break_when_profile_change');

  return (
    <SwitchItem
      label={m.settings_nyanpasu_enhance_break_when_profile_change_label()}
      checked={Boolean(breakWhenProfileChange.value)}
      onChange={() =>
        breakWhenProfileChange.upsert(!breakWhenProfileChange.value)
      }
    />
  );
};

const BreakWhenModeChange = () => {
  const breakWhenModeChange = useSetting('break_when_mode_change');

  return (
    <SwitchItem
      label={m.settings_nyanpasu_enhance_break_when_mode_change_label()}
      checked={Boolean(breakWhenModeChange.value)}
      onChange={() => breakWhenModeChange.upsert(!breakWhenModeChange.value)}
    />
  );
};

const TrayProxiesSelector = () => {
  const { value, upsert } = useSetting('clash_tray_selector');

  const trayProxiesSelectorMode = {
    normal: m.settings_nyanpasu_tray_type_normal(),
    hidden: m.settings_nyanpasu_tray_type_hidden(),
    submenu: m.settings_nyanpasu_tray_type_submenu(),
  };

  return (
    <MenuItem
      label={'Tray Proxies Selector'}
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
      label={m.settings_nyanpasu_enhance_enable_builtin_enhanced_label()}
      checked={Boolean(value)}
      onChange={() => upsert(!value)}
    />
  );
};

const LightenAnimationEffects = () => {
  const { value, upsert } = useSetting('lighten_animation_effects');

  return (
    <SwitchItem
      label={'Lighten Up Animation Effects'}
      checked={Boolean(value)}
      onChange={() => upsert(!value)}
    />
  );
};

export const SettingChimeraMisc = () => {
  return (
    <BaseCard label={m.settings_label_nyanpasu()}>
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
