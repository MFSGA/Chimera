import { createFileRoute } from '@tanstack/react-router';
import * as m from '@/paraglide/messages';
import { SettingsTitle } from '../_modules/settings-title';
import ChimeraSettings from './_modules/chimera-settings';

export const Route = createFileRoute('/(main)/main/settings/nyanpasu')({
  component: RouteComponent,
});

function RouteComponent() {
  return (
    <>
      <SettingsTitle>{m.settings_label_nyanpasu()}</SettingsTitle>
      <div className="space-y-4 px-4 pb-4">
        <ChimeraSettings />
      </div>
    </>
  );
}
