import { createFileRoute } from '@tanstack/react-router';
import * as m from '@/paraglide/messages';
import { SettingsTitle } from '../_modules/settings-title';
import UserInterfaceSettings from './_modules/user-interface-settings';

export const Route = createFileRoute('/(main)/main/settings/user-interface')({
  component: RouteComponent,
});

function RouteComponent() {
  return (
    <>
      <SettingsTitle>{m.settings_label_user_interface()}</SettingsTitle>

      <div className="space-y-4 px-4 pb-4">
        <UserInterfaceSettings />
      </div>
    </>
  );
}
