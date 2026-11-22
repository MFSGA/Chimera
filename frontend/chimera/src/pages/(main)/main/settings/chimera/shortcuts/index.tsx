import { createFileRoute } from '@tanstack/react-router';
import { PageTitle } from '@/components/page-title';
import * as m from '@/paraglide/messages';
import HotkeyManager from '../_modules/hotket-manager';
import { SettingsGroup } from '../../_modules/settings-card';

export const Route = createFileRoute(
  '/(main)/main/settings/chimera/shortcuts/',
)({
  component: RouteComponent,
  staticData: {
    crumb: {
      title: () => m.settings_chimera_keyboard_shortcuts(),
      parent: () => ({ to: '/main/settings' }),
    },
  },
});

function RouteComponent() {
  return (
    <>
      <PageTitle legacySlot="settings-title" />
      <div className="space-y-4 px-4 pb-4">
        <div data-slot="keyboard-settings-container">
          <SettingsGroup>
            <HotkeyManager />
          </SettingsGroup>
        </div>
      </div>
    </>
  );
}
