import { createFileRoute } from '@tanstack/react-router';
import { PageTitle } from '@/components/page-title';
import * as m from '@/paraglide/messages';
import LogLevelSelector from '../_modules/log-level-selector';
import ManagedGuardFieldSelector from '../_modules/managed-guard-field-selector';
import { SettingsGroup, SettingsLabel } from '../../_modules/settings-card';
import BreakWhenModeChangeSwitch from '../../chimera/_modules/break-when-mode-change-switch';
import BreakWhenProfileChangeSwitch from '../../chimera/_modules/break-when-profile-change-switch';
import BreakWhenProxyChangeSwitch from '../../chimera/_modules/break-when-proxy-change-switch';

export const Route = createFileRoute('/(main)/main/settings/clash/core/')({
  component: RouteComponent,
  staticData: {
    crumb: {
      title: () => m.settings_label_core_settings(),
      parent: () => ({ to: '/main/settings' }),
    },
  },
});

const CoreBehaviorSettings = () => (
  <div data-slot="core-behavior-settings-container">
    <SettingsLabel>{m.settings_clash_core_behavior_label()}</SettingsLabel>
    <SettingsGroup>
      <ManagedGuardFieldSelector field="unified-delay" />
      <ManagedGuardFieldSelector field="tcp-concurrent" />
      <LogLevelSelector />
    </SettingsGroup>
  </div>
);

const ConnectionSettings = () => (
  <div data-slot="connection-settings-container">
    <SettingsLabel>{m.settings_clash_core_behavior_label()}</SettingsLabel>
    <SettingsGroup>
      <BreakWhenProxyChangeSwitch />
      <BreakWhenProfileChangeSwitch />
      <BreakWhenModeChangeSwitch />
    </SettingsGroup>
  </div>
);

function RouteComponent() {
  return (
    <>
      <PageTitle legacySlot="settings-title" />
      <div className="space-y-4 px-4 pb-4">
        <CoreBehaviorSettings />
        <ConnectionSettings />
      </div>
    </>
  );
}
