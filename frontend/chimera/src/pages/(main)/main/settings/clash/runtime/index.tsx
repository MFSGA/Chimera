import { createFileRoute } from '@tanstack/react-router';
import * as m from '@/paraglide/messages';
import ExpandIncludeAllSwitch from '../_modules/expand-include-all-switch';
import FieldFilterCard from '../_modules/field-filter-card';
import FieldFilterSwitch from '../_modules/field-filter-switch';
import {
  SettingsCard,
  SettingsCardContent,
  SettingsGroup,
  SettingsLabel,
} from '../../_modules/settings-card';
import { SettingsTitle } from '../../_modules/settings-title';
import EnableBuiltinEnhancedSwitch from '../../chimera/_modules/enable-builtin-enhanced-switch';

export const Route = createFileRoute('/(main)/main/settings/clash/runtime/')({
  component: RouteComponent,
});

const FieldFilterSettings = () => (
  <div data-slot="field-filter-settings-container">
    <SettingsLabel>
      {m.settings_clash_settings_field_filter_label()}
    </SettingsLabel>
    <div className="space-y-2">
      <SettingsCard>
        <SettingsCardContent>
          <FieldFilterSwitch />
        </SettingsCardContent>
      </SettingsCard>
      <FieldFilterCard />
    </div>
  </div>
);

const ProfileProcessingSettings = () => (
  <div data-slot="profile-processing-settings-container">
    <SettingsLabel>{m.settings_clash_runtime_processing_label()}</SettingsLabel>
    <SettingsGroup>
      <ExpandIncludeAllSwitch />
      <EnableBuiltinEnhancedSwitch />
    </SettingsGroup>
  </div>
);

function RouteComponent() {
  return (
    <>
      <SettingsTitle>{m.settings_label_runtime()}</SettingsTitle>
      <div className="space-y-4 px-4 pb-4">
        <FieldFilterSettings />
        <ProfileProcessingSettings />
      </div>
    </>
  );
}
