import { createFileRoute } from '@tanstack/react-router';
import * as m from '@/paraglide/messages';
import { SettingsGroup, SettingsLabel } from '../../_modules/settings-card';
import { SettingsTitle } from '../../_modules/settings-title';
import CoreSecretConfig from '../../web-ui/_modules/core-secret-config';
import ExternalControllerConfig from '../../web-ui/_modules/external-controller-config';
import PortStrategySelector from '../../web-ui/_modules/port-strategy-selector';
import WebUI from '../../web-ui/_modules/web-ui';

export const Route = createFileRoute('/(main)/main/settings/clash/control/')({
  component: RouteComponent,
});

const ExternalController = () => (
  <div data-slot="theme-mode-settings-container">
    <SettingsLabel>{m.settings_label_external_controll()}</SettingsLabel>
    <SettingsGroup>
      <ExternalControllerConfig />
      <PortStrategySelector />
      <CoreSecretConfig />
    </SettingsGroup>
  </div>
);

function RouteComponent() {
  return (
    <>
      <SettingsTitle>{m.settings_label_control()}</SettingsTitle>
      <div className="space-y-4 px-4 pb-4">
        <ExternalController />
        <div data-slot="web-ui-settings-container">
          <SettingsLabel>{m.settings_web_ui_title()}</SettingsLabel>
          <WebUI />
        </div>
      </div>
    </>
  );
}
