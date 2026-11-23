import { createFileRoute } from '@tanstack/react-router';
import { PageTitle } from '@/components/page-title';
import * as m from '@/paraglide/messages';
import CoreManagerCard from '../_modules/core-manager-card';
import { SettingsGroup } from '../../_modules/settings-card';

export const Route = createFileRoute(
  '/(main)/main/settings/clash/core-manager/',
)({
  component: RouteComponent,
  staticData: {
    crumb: {
      title: () => m.settings_label_core_manager(),
      parent: () => ({ to: '/main/settings' }),
    },
  },
});

function RouteComponent() {
  return (
    <>
      <PageTitle legacySlot="settings-title" />
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
