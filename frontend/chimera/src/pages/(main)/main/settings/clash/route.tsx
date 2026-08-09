import { createFileRoute } from '@tanstack/react-router';
import * as m from '@/paraglide/messages';
import { SettingsGroup, SettingsLabel } from '../_modules/settings-card';
import { SettingsTitle } from '../_modules/settings-title';
import {
  AllowLanSwitch,
  IPv6Switch,
  LogLevelSelector,
  TunStackSelector,
  UWPTool,
} from './_modules/base-settings';
import CoreManager from './_modules/core-manager';
import { FieldFilterCard, FieldFilterSwitch } from './_modules/field-filter';
import { MixedPortConfig, RandomPortSwitch } from './_modules/port-settings';

export const Route = createFileRoute('/(main)/main/settings/clash')({
  component: RouteComponent,
});

const PatchSettings = () => (
  <section data-slot="patch-settings-container">
    <SettingsLabel>{m.settings_clash_settings_title()}</SettingsLabel>
    <SettingsGroup>
      <AllowLanSwitch />
      <IPv6Switch />
      <TunStackSelector />
      <LogLevelSelector />
      <UWPTool />
    </SettingsGroup>
  </section>
);

const PortSettings = () => (
  <section data-slot="port-settings-container">
    <SettingsLabel>{m.settings_clash_settings_port_label()}</SettingsLabel>
    <SettingsGroup>
      <MixedPortConfig />
      <RandomPortSwitch />
    </SettingsGroup>
  </section>
);

const CoreManagerSettings = () => (
  <section data-slot="core-manager-settings-container">
    <SettingsLabel>{m.settings_clash_core_manager_card_title()}</SettingsLabel>
    <SettingsGroup>
      <CoreManager />
    </SettingsGroup>
  </section>
);

const FieldFilterSettings = () => (
  <section data-slot="field-filter-settings-container">
    <SettingsLabel>
      {m.settings_clash_settings_field_filter_label()}
    </SettingsLabel>
    <div className="space-y-2">
      <SettingsGroup>
        <FieldFilterSwitch />
      </SettingsGroup>
      <FieldFilterCard />
    </div>
  </section>
);

function RouteComponent() {
  return (
    <>
      <SettingsTitle>{m.settings_clash_settings_title()}</SettingsTitle>
      <div className="space-y-4 px-4 pb-4">
        <PatchSettings />
        <PortSettings />
        <CoreManagerSettings />
        <FieldFilterSettings />
      </div>
    </>
  );
}
