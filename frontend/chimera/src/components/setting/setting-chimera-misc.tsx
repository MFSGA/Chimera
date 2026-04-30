import {
  useSetting,
  type BreakWhenProxyChange as BreakWhenProxyChangeType,
  type LoggingLevel,
} from '@chimera/interface';
import { BaseCard, MenuItem, SwitchItem } from '@chimera/ui';
import { List } from '@mui/material';
import { useLockFn } from 'ahooks';
import { useTranslation } from 'react-i18next';
import { formatError } from '@/utils';
import { message } from '@/utils/notification';

const AppLogLevel = () => {
  const { t } = useTranslation();
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
      label={t('App Log Level')}
      options={options}
      selected={value || 'info'}
      onSelected={(value) => upsert(value as LoggingLevel)}
    />
  );
};

const BreakWhenProxyChange = () => {
  const { t } = useTranslation();
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
        title: t('Error'),
        kind: 'error',
      });
    }
  });

  return (
    <SwitchItem
      label={t('Disconnect on Proxy Change')}
      checked={checked}
      onChange={handleChange}
    />
  );
};

const BreakWhenProfileChange = () => {
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

const BreakWhenModeChange = () => {
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

const EnableBuiltinEnhanced = () => {
  const { t } = useTranslation();
  const { value, upsert } = useSetting('enable_builtin_enhanced');

  return (
    <SwitchItem
      label={t('Enable Built-in Enhanced')}
      checked={Boolean(value)}
      onChange={() => upsert(!value)}
    />
  );
};

export const SettingChimeraMisc = () => {
  const { t } = useTranslation();

  return (
    <BaseCard label={t('Nyanpasu Setting')}>
      <List disablePadding>
        <AppLogLevel />

        <BreakWhenProxyChange />

        <BreakWhenProfileChange />

        <BreakWhenModeChange />

        <EnableBuiltinEnhanced />
      </List>
    </BaseCard>
  );
};

export default SettingChimeraMisc;
