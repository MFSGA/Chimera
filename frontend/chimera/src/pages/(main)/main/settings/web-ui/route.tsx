import { createFileRoute } from '@tanstack/react-router';
import { PageTitle } from '@/components/page-title';
import * as m from '@/paraglide/messages';
import { SettingsGroup, SettingsLabel } from '../_modules/settings-card';
import CoreSecretConfig from './_modules/core-secret-config';
import ExternalControllerConfig from './_modules/external-controller-config';
import PortStrategySelector from './_modules/port-strategy-selector';
import WebUI from './_modules/web-ui';

export const Route = createFileRoute('/(main)/main/settings/web-ui')({
  component: RouteComponent,
  staticData: { crumb: { title: () => m.settings_label_external_controll() } },
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

const WebUISettings = () => (
  <div data-slot="theme-mode-settings-container">
    <SettingsLabel>{m.settings_web_ui_title()}</SettingsLabel>
    <WebUI />
  </div>
);

function RouteComponent() {
  return (
    <>
      <PageTitle legacySlot="settings-title" />
      <div className="space-y-4 px-4 pb-4">
        <ExternalController />
        <WebUISettings />
      </div>
    </>
  );
}
