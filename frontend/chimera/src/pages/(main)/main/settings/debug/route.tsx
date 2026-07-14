import { createFileRoute } from '@tanstack/react-router';
import SettingChimeraPath from '@/components/setting/setting-chimera-path';
import * as m from '@/paraglide/messages';
import { SettingsTitle } from '../_modules/settings-title';

export const Route = createFileRoute('/(main)/main/settings/debug')({
  component: RouteComponent,
});

function RouteComponent() {
  return (
    <>
      <SettingsTitle>{m.settings_label_debug()}</SettingsTitle>

      <div className="grid gap-4 px-4 pb-4">
        <SettingChimeraPath />
      </div>
    </>
  );
}
