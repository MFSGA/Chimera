import {
  useSetting,
  type BreakWhenProxyChange as BreakWhenProxyChangeType,
  type LoggingLevel_Serialize,
  type ProxiesSelectorMode,
} from '@chimera/interface';
import { AnimatePresence } from 'motion/react';
import { useEffect, useState } from 'react';
import { Button } from '@/components/ui/button';
import { NumericInput } from '@/components/ui/input';
import * as m from '@/paraglide/messages';
import { formatError } from '@/utils';
import { message } from '@/utils/notification';
import {
  SettingsCard,
  SettingsCardAnimatedItem,
  SettingsCardContent,
  SettingsGroup,
  SettingsLabel,
} from '../../_modules/settings-card';
import { SelectorCard, SwitchCard } from './setting-control';

const AppLogLevel = () => {
  const setting = useSetting('app_log_level');
  const options: Record<LoggingLevel_Serialize, string> = {
    trace: 'Trace',
    debug: 'Debug',
    info: 'Info',
    warn: 'Warn',
    error: 'Error',
    silent: 'Silent',
  };

  const handleSelect = async (value: LoggingLevel_Serialize) => {
    try {
      await setting.upsert(value);
    } catch (error) {
      message(
        `${m.settings_nyanpasu_app_log_level_label()}\n${formatError(error)}`,
        { title: m.common_error(), kind: 'error' },
      );
    }
  };

  return (
    <SelectorCard
      id="verge-app-log-level"
      label={m.settings_nyanpasu_app_log_level_label()}
      current={setting.value || 'info'}
      options={options}
      onSelect={(value) => void handleSelect(value)}
    />
  );
};

const TrayProxiesSelector = () => {
  const setting = useSetting('clash_tray_selector');
  const options: Record<ProxiesSelectorMode, string> = {
    normal: m.settings_nyanpasu_tray_type_normal(),
    hidden: m.settings_nyanpasu_tray_type_hidden(),
    submenu: m.settings_nyanpasu_tray_type_submenu(),
  };

  return (
    <SelectorCard
      label={m.settings_nyanpasu_proxies_selector_label()}
      current={setting.value || 'normal'}
      options={options}
      onSelect={(value) => void setting.upsert(value)}
    />
  );
};

const BreakWhenProxyChange = () => {
  const setting = useSetting('break_when_proxy_change');
  const checked = Boolean(setting.value && setting.value !== 'none');

  const handleChange = async (nextChecked: boolean) => {
    try {
      await setting.upsert(
        (nextChecked ? 'all' : 'none') as BreakWhenProxyChangeType,
      );
    } catch (error) {
      message(
        `${m.settings_proxies_break_change_update_failed()}\n${formatError(error)}`,
        { title: m.common_error(), kind: 'error' },
      );
    }
  };

  return (
    <SwitchCard
      label={m.settings_nyanpasu_enhance_break_when_proxy_change_label()}
      checked={checked}
      loading={setting.isPending}
      onCheckedChange={(value) => void handleChange(value)}
    />
  );
};

const BreakWhenProfileChange = () => {
  const setting = useSetting('break_when_profile_change');
  return (
    <SwitchCard
      label={m.settings_nyanpasu_enhance_break_when_profile_change_label()}
      checked={Boolean(setting.value)}
      loading={setting.isPending}
      onCheckedChange={(value) => void setting.upsert(value)}
    />
  );
};

const BreakWhenModeChange = () => {
  const setting = useSetting('break_when_mode_change');
  return (
    <SwitchCard
      label={m.settings_nyanpasu_enhance_break_when_mode_change_label()}
      checked={Boolean(setting.value)}
      loading={setting.isPending}
      onCheckedChange={(value) => void setting.upsert(value)}
    />
  );
};

const EnableBuiltinEnhanced = () => {
  const setting = useSetting('enable_builtin_enhanced');
  return (
    <SwitchCard
      label={m.settings_nyanpasu_enhance_enable_builtin_enhanced_label()}
      checked={Boolean(setting.value)}
      loading={setting.isPending}
      onCheckedChange={(value) => void setting.upsert(value)}
    />
  );
};

const LightenAnimationEffects = () => {
  const setting = useSetting('lighten_animation_effects');
  return (
    <SwitchCard
      label={m.settings_nyanpasu_lighten_animations_label()}
      checked={Boolean(setting.value)}
      loading={setting.isPending}
      onCheckedChange={(value) => void setting.upsert(value)}
    />
  );
};

const MaxLogFiles = () => {
  const setting = useSetting('max_log_files');
  const savedValue = setting.value ?? 7;
  const [draft, setDraft] = useState<number | null>(savedValue);

  useEffect(() => setDraft(savedValue), [savedValue]);

  const isDirty = draft !== savedValue;
  const isValid = draft != null && Number.isInteger(draft) && draft >= 1;

  return (
    <SettingsCard>
      <SettingsCardContent>
        <NumericInput
          variant="outlined"
          label={m.settings_nyanpasu_max_log_files_label()}
          value={draft}
          min={1}
          allowNegative={false}
          decimalScale={0}
          onChange={setDraft}
        />

        <AnimatePresence initial={false}>
          {isDirty && (
            <SettingsCardAnimatedItem>
              <div className="flex justify-end gap-2">
                <Button onClick={() => setDraft(savedValue)}>
                  {m.common_reset()}
                </Button>
                <Button
                  variant="raised"
                  disabled={!isValid}
                  loading={setting.isPending}
                  onClick={() => draft != null && void setting.upsert(draft)}
                >
                  {m.common_apply()}
                </Button>
              </div>
            </SettingsCardAnimatedItem>
          )}
        </AnimatePresence>
      </SettingsCardContent>
    </SettingsCard>
  );
};

export default function ChimeraSettings() {
  return (
    <>
      <div data-slot="app-settings-container">
        <SettingsLabel>{m.settings_nyanpasu_logs()}</SettingsLabel>
        <SettingsGroup>
          <AppLogLevel />
          <MaxLogFiles />
        </SettingsGroup>
      </div>

      <div data-slot="app-settings-container">
        <SettingsLabel>{m.settings_nyanpasu_enhance_label()}</SettingsLabel>
        <SettingsGroup>
          <BreakWhenProxyChange />
          <BreakWhenProfileChange />
          <BreakWhenModeChange />
          <EnableBuiltinEnhanced />
        </SettingsGroup>
      </div>

      <div data-slot="app-settings-container">
        <SettingsLabel>{m.settings_nyanpasu_tray()}</SettingsLabel>
        <SettingsGroup>
          <TrayProxiesSelector />
        </SettingsGroup>
      </div>

      <div data-slot="app-settings-container">
        <SettingsLabel>{m.settings_label_user_interface()}</SettingsLabel>
        <SettingsGroup>
          <LightenAnimationEffects />
        </SettingsGroup>
      </div>
    </>
  );
}
