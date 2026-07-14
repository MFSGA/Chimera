import { createFileRoute } from '@tanstack/react-router';
import { SettingClashBase } from '@/components/setting/setting-clash-base';
import SettingClashCore from '@/components/setting/setting-clash-core';
import SettingClashField from '@/components/setting/setting-clash-field';
import SettingClashPort from '@/components/setting/setting-clash-port';
import * as m from '@/paraglide/messages';
import { SettingsTitle } from '../_modules/settings-title';

export const Route = createFileRoute('/(main)/main/settings/clash')({
  component: RouteComponent,
});

function RouteComponent() {
  return (
    <>
      <SettingsTitle>{m.settings_label_clash_settings()}</SettingsTitle>

      <div className="grid gap-4 px-4 pb-4 xl:grid-cols-2">
        <SettingClashBase />
        <SettingClashPort />
        <SettingClashCore />
        <SettingClashField />
      </div>
    </>
  );
}
