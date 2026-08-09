import { useIsAppImage } from '@chimera/interface';
import { getSystem } from '@chimera/ui';
import { createFileRoute } from '@tanstack/react-router';
import * as m from '@/paraglide/messages';
import { SettingsTitle } from '../_modules/settings-title';
import SystemBehavior from './_modules/system-behavior';
import SystemProxy from './_modules/system-proxy';
import SystemService from './_modules/system-service';
import SystemTools from './_modules/system-tools';

const OS = getSystem();

export const Route = createFileRoute('/(main)/main/settings/system')({
  component: RouteComponent,
});

function RouteComponent() {
  const isAppImage = useIsAppImage();

  return (
    <>
      <SettingsTitle>{m.settings_label_system()}</SettingsTitle>

      <div className="space-y-4 px-4 pb-4">
        <SystemProxy />
        {!isAppImage.data && <SystemService />}
        <SystemBehavior />
        {OS === 'windows' && <SystemTools />}
      </div>
    </>
  );
}
