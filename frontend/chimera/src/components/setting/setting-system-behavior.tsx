import { useSetting, type BreakWhenProxyChange } from '@chimera/interface';
import { BaseCard, MenuItem, SwitchItem } from '@chimera/ui';
import { List } from '@mui/material';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import { useTranslation } from 'react-i18next';

const appWindow = getCurrentWebviewWindow();

const AlwaysOnTopButton = () => {
  const { t } = useTranslation();
  const alwaysOnTop = useSetting('always_on_top');

  const handleToggle = async () => {
    const nextValue = !alwaysOnTop.value;
    await alwaysOnTop.upsert(nextValue);
    await appWindow.setAlwaysOnTop(nextValue);
  };

  return (
    <SwitchItem
      label={t('Always On Top')}
      checked={Boolean(alwaysOnTop.value)}
      onChange={handleToggle}
    />
  );
};

const AutoCheckUpdateButton = () => {
  const { t } = useTranslation();
  const autoCheckUpdate = useSetting('enable_auto_check_update');

  return (
    <SwitchItem
      label={t('Auto Check Updates')}
      checked={autoCheckUpdate.value ?? true}
      onChange={() => autoCheckUpdate.upsert(!(autoCheckUpdate.value ?? true))}
    />
  );
};

const BreakWhenProfileChangeButton = () => {
  const { t } = useTranslation();
  const breakWhenProfileChange = useSetting('break_when_profile_change');

  return (
    <SwitchItem
      label={t('Disconnect on Profile Change')}
      checked={Boolean(breakWhenProfileChange.value)}
      onChange={() =>
        breakWhenProfileChange.upsert(!breakWhenProfileChange.value)
      }
    />
  );
};

const BreakWhenModeChangeButton = () => {
  const { t } = useTranslation();
  const breakWhenModeChange = useSetting('break_when_mode_change');

  return (
    <SwitchItem
      label={t('Disconnect on Mode Change')}
      checked={Boolean(breakWhenModeChange.value)}
      onChange={() => breakWhenModeChange.upsert(!breakWhenModeChange.value)}
    />
  );
};

const BreakWhenProxyChangeItem = () => {
  const { t } = useTranslation();
  const breakWhenProxyChange = useSetting('break_when_proxy_change');

  return (
    <MenuItem
      label={t('Disconnect on Proxy Change')}
      selectSx={{ width: 180 }}
      selected={breakWhenProxyChange.value ?? 'none'}
      options={{
        none: t('Keep Connections'),
        chain: t('Disconnect Active Chain'),
        all: t('Disconnect All Connections'),
      }}
      onSelected={(value) =>
        breakWhenProxyChange.upsert(value as BreakWhenProxyChange)
      }
    />
  );
};

export const SettingSystemBehavior = () => {
  const { t } = useTranslation();

  return (
    <BaseCard label={t('System Behavior')}>
      <List disablePadding>
        <AlwaysOnTopButton />
        <AutoCheckUpdateButton />
        <BreakWhenProfileChangeButton />
        <BreakWhenModeChangeButton />
        <BreakWhenProxyChangeItem />
      </List>
    </BaseCard>
  );
};

export default SettingSystemBehavior;
