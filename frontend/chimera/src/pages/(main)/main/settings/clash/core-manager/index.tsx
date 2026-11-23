import { createFileRoute } from '@tanstack/react-router';
import * as m from '@/paraglide/messages';
import CoreManagerCard from '../_modules/core-manager-card';
import { SettingsGroup } from '../../_modules/settings-card';
import { SettingsTitle } from '../../_modules/settings-title';

export const Route = createFileRoute(
  '/(main)/main/settings/clash/core-manager/',
)({
  component: RouteComponent,
});

function RouteComponent() {
  return (
    <>
      <SettingsTitle>{m.settings_label_core_manager()}</SettingsTitle>
      <div className="space-y-4 px-4 pb-4">
        <div data-slot="core-manager-settings-container">
          <SettingsGroup>
            <CoreManagerCard />
          </SettingsGroup>
        </div>
      </div>
    </>
  );
}
