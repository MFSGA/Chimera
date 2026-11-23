import { createFileRoute } from '@tanstack/react-router';
import * as m from '@/paraglide/messages';
import HotkeyManager from '../_modules/hotket-manager';
import { SettingsGroup } from '../../_modules/settings-card';
import { SettingsTitle } from '../../_modules/settings-title';

export const Route = createFileRoute(
  '/(main)/main/settings/chimera/shortcuts/',
)({
  component: RouteComponent,
});

function RouteComponent() {
  return (
    <>
      <SettingsTitle>{m.settings_chimera_keyboard_shortcuts()}</SettingsTitle>
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
