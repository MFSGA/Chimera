import { createFileRoute } from '@tanstack/react-router';
import SettingSystemBehavior from '@/components/setting/setting-system-behavior';
import SettingSystemProxy from '@/components/setting/setting-system-proxy';
import SettingSystemService from '@/components/setting/setting-system-service';
import { useIsAppImage } from '@/hooks/use-consts';
import * as m from '@/paraglide/messages';
import { SettingsTitle } from '../_modules/settings-title';

export const Route = createFileRoute('/(main)/main/settings/system')({
  component: RouteComponent,
});

function RouteComponent() {
  const isAppImage = useIsAppImage();

  return (
    <>
      <SettingsTitle>{m.settings_label_system()}</SettingsTitle>

      <div className="grid gap-4 px-4 pb-4 xl:grid-cols-2">
        <SettingSystemProxy />
        <SettingSystemBehavior />
        {!isAppImage.data && <SettingSystemService />}
      </div>
    </>
  );
}
