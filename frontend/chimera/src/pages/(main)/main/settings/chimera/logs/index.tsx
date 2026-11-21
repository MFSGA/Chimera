import { createFileRoute } from '@tanstack/react-router';
import * as m from '@/paraglide/messages';
import LogFileConfig from '../_modules/log-file-config';
import AppLogLevelSelector from '../_modules/log-level-selector';
import TrafficRetentionSelector from '../_modules/traffic-retention-selector';
import { SettingsGroup, SettingsLabel } from '../../_modules/settings-card';
import { SettingsTitle } from '../../_modules/settings-title';
import CoreLogLevelSelector from '../../clash/_modules/log-level-selector';

export const Route = createFileRoute('/(main)/main/settings/chimera/logs/')({
  component: RouteComponent,
});

const AppLogSettings = () => (
  <div data-slot="app-log-settings-container">
    <SettingsLabel>{m.settings_logs_app_label()}</SettingsLabel>
    <SettingsGroup>
      <AppLogLevelSelector />
      <LogFileConfig />
    </SettingsGroup>
  </div>
);

const CoreLogSettings = () => (
  <div data-slot="core-log-settings-container">
    <SettingsLabel>{m.settings_logs_core_label()}</SettingsLabel>
    <SettingsGroup>
      <CoreLogLevelSelector />
    </SettingsGroup>
  </div>
);

const TrafficSettings = () => (
  <div data-slot="traffic-settings-container">
    <SettingsLabel>{m.settings_chimera_traffic_label()}</SettingsLabel>
    <SettingsGroup>
      <TrafficRetentionSelector />
    </SettingsGroup>
  </div>
);

function RouteComponent() {
  return (
    <>
      <SettingsTitle>{m.settings_label_logs()}</SettingsTitle>
      <div className="space-y-4 px-4 pb-4">
        <AppLogSettings />
        <CoreLogSettings />
        <TrafficSettings />
      </div>
    </>
  );
}
