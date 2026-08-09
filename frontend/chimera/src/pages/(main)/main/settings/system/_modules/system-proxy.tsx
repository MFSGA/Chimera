import { useSetting } from '@chimera/interface';
import { AnimatePresence } from 'motion/react';
import {
  SystemProxyButton,
  TunModeButton,
} from '@/components/settings/system-proxy';
import * as m from '@/paraglide/messages';
import {
  SettingsCard,
  SettingsCardAnimatedItem,
  SettingsCardContent,
  SettingsGroup,
  SettingsLabel,
} from '../../_modules/settings-card';
import CurrentSystemProxy from './current-system-proxy';
import ProxyBypassConfig from './proxy-bypass-config';
import ProxyGuardConfig from './proxy-guard-config';
import ProxyGuardSwitch from './proxy-guard-switch';
import ProxyModeSelector from './proxy-mode-selector';

const ProxyMode = () => (
  <section data-slot="proxy-mode-container">
    <SettingsLabel>{m.settings_system_proxy_proxy_mode_label()}</SettingsLabel>
    <SettingsGroup className="pb-4">
      <div className="grid grid-cols-2 gap-2">
        <SystemProxyButton />
        <TunModeButton />
      </div>
    </SettingsGroup>
    <ProxyModeSelector />
  </section>
);

const ProxyGuard = () => {
  const { value } = useSetting('enable_proxy_guard');

  return (
    <section data-slot="proxy-guard-container">
      <SettingsLabel>
        {m.settings_system_proxy_proxy_guard_label()}
      </SettingsLabel>
      <SettingsGroup>
        <SettingsCard>
          <SettingsCardContent>
            <ProxyGuardSwitch />
          </SettingsCardContent>
        </SettingsCard>
        <AnimatePresence initial={false}>
          {value && (
            <SettingsCard asChild>
              <SettingsCardAnimatedItem>
                <SettingsCardContent>
                  <ProxyGuardConfig />
                  <ProxyBypassConfig />
                </SettingsCardContent>
              </SettingsCardAnimatedItem>
            </SettingsCard>
          )}
        </AnimatePresence>
      </SettingsGroup>
    </section>
  );
};

const CurrentProxy = () => (
  <section data-slot="current-system-proxy-container">
    <SettingsLabel>
      {m.settings_system_proxy_current_system_proxy_label()}
    </SettingsLabel>
    <SettingsGroup>
      <SettingsCard>
        <SettingsCardContent className="py-4">
          <CurrentSystemProxy />
        </SettingsCardContent>
      </SettingsCard>
    </SettingsGroup>
  </section>
);

export default function SystemProxy() {
  return (
    <>
      <ProxyMode />
      <ProxyGuard />
      <CurrentProxy />
    </>
  );
}
