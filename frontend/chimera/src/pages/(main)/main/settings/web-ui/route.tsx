import { createFileRoute } from '@tanstack/react-router';
import * as m from '@/paraglide/messages';
import { SettingsGroup, SettingsLabel } from '../_modules/settings-card';
import { SettingsTitle } from '../_modules/settings-title';
import CoreSecretConfig from './_modules/core-secret-config';
import ExternalControllerConfig from './_modules/external-controller-config';
import PortStrategySelector from './_modules/port-strategy-selector';
import WebUI from './_modules/web-ui';

export const Route = createFileRoute('/(main)/main/settings/web-ui')({
  component: RouteComponent,
});

const ExternalController = () => (
  <section data-slot="external-controller-settings-container">
    <SettingsLabel>{m.settings_label_external_controll()}</SettingsLabel>
    <SettingsGroup>
      <ExternalControllerConfig />
      <PortStrategySelector />
      <CoreSecretConfig />
    </SettingsGroup>
  </section>
);

const WebUISettings = () => (
  <section data-slot="web-ui-settings-container">
    <SettingsLabel>{m.settings_web_ui_title()}</SettingsLabel>
    <WebUI />
  </section>
);

function RouteComponent() {
  return (
    <>
      <SettingsTitle>{m.settings_label_external_controll()}</SettingsTitle>
      <div className="space-y-4 px-4 pb-4">
        <ExternalController />
        <WebUISettings />
      </div>
    </>
  );
}
