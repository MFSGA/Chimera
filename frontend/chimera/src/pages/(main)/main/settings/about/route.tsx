import { createFileRoute } from '@tanstack/react-router';
import z from 'zod';
import * as m from '@/paraglide/messages';
import { SettingsTitle } from '../_modules/settings-title';
import ChimeraVersion from './_modules/chimera-version';

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

      <div className="space-y-4 px-4 pb-4">
        <div className="grid gap-2 sm:grid-cols-2">
          <ChimeraVersion />
        </div>
      </div>
    </>
  );
}
