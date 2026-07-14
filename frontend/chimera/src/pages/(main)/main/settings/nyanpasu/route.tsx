import { createFileRoute } from '@tanstack/react-router';
import SettingChimeraMisc from '@/components/setting/setting-chimera-misc';
import SettingChimeraTasks from '@/components/setting/setting-chimera-tasks';
import * as m from '@/paraglide/messages';
import { SettingsTitle } from '../_modules/settings-title';

export const Route = createFileRoute('/(main)/main/settings/nyanpasu')({
  component: RouteComponent,
});

function RouteComponent() {
  return (
    <>
      <SettingsTitle>{m.settings_label_nyanpasu()}</SettingsTitle>

      <div className="grid gap-4 px-4 pb-4 xl:grid-cols-2">
        <SettingChimeraMisc />
        <SettingChimeraTasks />
      </div>
    </>
  );
}
