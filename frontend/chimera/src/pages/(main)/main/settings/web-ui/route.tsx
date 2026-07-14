import { createFileRoute } from '@tanstack/react-router';
import SettingClashExternal from '@/components/setting/setting-clash-external';
import SettingClashWeb from '@/components/setting/setting-clash-web';
import * as m from '@/paraglide/messages';
import { SettingsTitle } from '../_modules/settings-title';

export const Route = createFileRoute('/(main)/main/settings/web-ui')({
  component: RouteComponent,
});

function RouteComponent() {
  return (
    <>
      <SettingsTitle>{m.settings_label_external_controll()}</SettingsTitle>

      <div className="grid gap-4 px-4 pb-4 xl:grid-cols-2">
        <SettingClashExternal />
        <SettingClashWeb />
      </div>
    </>
  );
}
