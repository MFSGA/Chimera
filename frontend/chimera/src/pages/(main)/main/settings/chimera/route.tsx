import { createFileRoute, Outlet, useMatchRoute } from '@tanstack/react-router';
import * as m from '@/paraglide/messages';
import { SettingsGroup, SettingsLabel } from '../_modules/settings-card';
import { SettingsTitle } from '../_modules/settings-title';
import BreakWhenModeChangeSwitch from './_modules/break-when-mode-change-switch';
import BreakWhenProfileChangeSwitch from './_modules/break-when-profile-change-switch';
import BreakWhenProxyChangeSwitch from './_modules/break-when-proxy-change-switch';
import EnableBuiltinEnhancedSwitch from './_modules/enable-builtin-enhanced-switch';
import HotkeyManager from './_modules/hotket-manager';
import LightenAnimationEffectsSwitch from './_modules/lighten-animation-effects-switch';
import LocalIpProbeSwitch from './_modules/local-ip-probe-switch';
import LogFileConfig from './_modules/log-file-config';
import LogLevelSelector from './_modules/log-level-selector';
import IpLookupProviderList from './_modules/public-ip/ip-lookup-provider-list';
import StunServerList from './_modules/public-ip/stun-server-list';
import TrafficRetentionSelector from './_modules/traffic-retention-selector';
import TrayProxiesSelector from './_modules/tray-proxies-selector';
import WindowCloseBehaviorCards from './_modules/window-close-behavior';

export const Route = createFileRoute('/(main)/main/settings/chimera')({
  component: RouteComponent,
});

const LogSettings = () => {
  return (
    <div data-slot="app-settings-container">
      <SettingsLabel>{m.settings_chimera_logs()}</SettingsLabel>

      <SettingsGroup>
        <LogLevelSelector />

        <LogFileConfig />
      </SettingsGroup>
    </div>
  );
};

const TrafficSettings = () => {
  return (
    <div data-slot="app-settings-container">
      <SettingsLabel>{m.settings_chimera_traffic_label()}</SettingsLabel>

      <SettingsGroup>
        <TrafficRetentionSelector />
      </SettingsGroup>
    </div>
  );
};

const PublicIpSettings = () => (
  <div
    className="space-y-4"
    data-slot="public-ip-settings"
    id="public-ip-settings"
  >
    <SettingsLabel>{m.settings_label_public_ip()}</SettingsLabel>
    <SettingsGroup>
      <LocalIpProbeSwitch />
    </SettingsGroup>
    <StunServerList />
    <IpLookupProviderList />
  </div>
);

const EnhanceSettings = () => {
  return (
    <div data-slot="app-settings-container">
      <SettingsLabel>{m.settings_chimera_enhance_label()}</SettingsLabel>

      <SettingsGroup>
        <BreakWhenProxyChangeSwitch />

        <BreakWhenProfileChangeSwitch />

        <BreakWhenModeChangeSwitch />

        <EnableBuiltinEnhancedSwitch />
      </SettingsGroup>
    </div>
  );
};

const TraySettings = () => {
  return (
    <div data-slot="app-settings-container">
      <SettingsLabel>{m.settings_chimera_tray()}</SettingsLabel>

      <SettingsGroup>
        <TrayProxiesSelector />
      </SettingsGroup>
    </div>
  );
};

const UserInterfaceSettings = () => {
  return (
    <div data-slot="app-settings-container">
      <SettingsLabel>{m.settings_label_user_interface()}</SettingsLabel>

      <SettingsGroup>
        <LightenAnimationEffectsSwitch />
      </SettingsGroup>

      <SettingsLabel>{m.settings_chimera_window_close()}</SettingsLabel>
      <SettingsGroup>
        <WindowCloseBehaviorCards />
      </SettingsGroup>
    </div>
  );
};

const HotkeySettings = () => {
  return (
    <div data-slot="app-settings-container">
      <SettingsLabel>{m.settings_chimera_keyboard_shortcuts()}</SettingsLabel>

      <SettingsGroup>
        <HotkeyManager />
      </SettingsGroup>
    </div>
  );
};

function RouteComponent() {
  const matchRoute = useMatchRoute();

  // The prior all-in-one route stays available to existing UI and E2E links.
  if (!matchRoute({ to: '/main/settings/chimera', fuzzy: false })) {
    return <Outlet />;
  }

  return (
    <>
      <SettingsTitle>{m.settings_label_chimera()}</SettingsTitle>

      <div className="space-y-4 px-4 pb-4">
        <LogSettings />

        <TrafficSettings />

        <PublicIpSettings />

        <EnhanceSettings />

        <TraySettings />

        <HotkeySettings />

        <UserInterfaceSettings />
      </div>
    </>
  );
}
