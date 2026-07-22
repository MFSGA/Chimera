import { createFileRoute } from '@tanstack/react-router';
import z from 'zod';
import SettingChimeraVersion from '@/components/setting/setting-chimera-version';
import * as m from '@/paraglide/messages';
import { SettingsTitle } from '../_modules/settings-title';

export enum Action {
  NEED_UPDATE = 'need-update',
}

export const Route = createFileRoute('/(main)/main/settings/about')({
  component: RouteComponent,
  validateSearch: z.object({
    action: z.enum(Action).optional().nullable(),
  }),
});

function RouteComponent() {
  return (
    <>
      <SettingsTitle>{m.settings_label_about()}</SettingsTitle>

      <div className="grid gap-4 px-4 pb-4">
        <SettingChimeraVersion />
      </div>
    </>
  );
}
