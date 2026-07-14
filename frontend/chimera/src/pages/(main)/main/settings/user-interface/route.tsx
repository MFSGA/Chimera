import { createFileRoute } from '@tanstack/react-router';
import SettingChimeraUI from '@/components/setting/setting-chimera-ui';
import * as m from '@/paraglide/messages';
import { SettingsTitle } from '../_modules/settings-title';

export const Route = createFileRoute('/(main)/main/settings/user-interface')({
  component: RouteComponent,
});

function RouteComponent() {
  return (
    <>
      <SettingsTitle>{m.settings_label_user_interface()}</SettingsTitle>

      <div className="grid gap-4 px-4 pb-4 xl:grid-cols-2">
        <SettingChimeraUI />
      </div>
    </>
  );
}
