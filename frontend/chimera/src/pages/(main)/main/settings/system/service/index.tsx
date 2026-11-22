import { useIsAppImage } from '@chimera/interface';
import { createFileRoute } from '@tanstack/react-router';
import { PageTitle } from '@/components/page-title';
import { OS } from '@/consts';
import * as m from '@/paraglide/messages';
import AutoLaunchSwitch from '../_modules/auto-launch-switch';
import DnsCacheButton from '../_modules/dns-cache-button';
import SilentLaunchSwitch from '../_modules/slient-launch-switch';
import SystemServiceCtrl from '../_modules/system-service-ctrl';
import SystemServiceSwitch from '../_modules/system-service-switch';
import UwpToolsButton from '../_modules/uwp-tools-button';
import {
  SettingsCard,
  SettingsCardContent,
  SettingsGroup,
  SettingsLabel,
} from '../../_modules/settings-card';

export const Route = createFileRoute('/(main)/main/settings/system/service/')({
  component: RouteComponent,
  staticData: {
    crumb: {
      title: () => m.settings_label_service(),
      parent: () => ({ to: '/main/settings' }),
    },
  },
});

const SystemService = () => {
  return (
    <div data-slot="system-service-container">
      <SettingsLabel>
        {m.settings_system_proxy_system_service_ctrl_label()}
      </SettingsLabel>

      <SettingsGroup>
        <SettingsCard>
          <SettingsCardContent>
            <SystemServiceSwitch />
          </SettingsCardContent>
        </SettingsCard>

        <SystemServiceCtrl />
      </SettingsGroup>
    </div>
  );
};

const SystemLaunch = () => {
  return (
    <div data-slot="system-launch-container">
      <SettingsLabel>{m.settings_system_proxy_launch_label()}</SettingsLabel>

      <SettingsGroup>
        <SettingsCard>
          <SettingsCardContent>
            <AutoLaunchSwitch />
          </SettingsCardContent>
        </SettingsCard>

        <SettingsCard>
          <SettingsCardContent>
            <SilentLaunchSwitch />
          </SettingsCardContent>
        </SettingsCard>
      </SettingsGroup>
    </div>
  );
};

const SystemTools = () => {
  return (
    <div data-slot="system-tools-container">
      <SettingsLabel>
        {m.settings_system_proxy_system_tools_label()}
      </SettingsLabel>

      <SettingsGroup>
        <DnsCacheButton />

        {OS === 'windows' && <UwpToolsButton />}
      </SettingsGroup>
    </div>
  );
};

function RouteComponent() {
  const isAppImage = useIsAppImage();

  return (
    <>
      <PageTitle legacySlot="settings-title" />

      <div className="space-y-4 px-4 pb-4">
        {!isAppImage.data && <SystemService />}

        <SystemLaunch />

        {(OS === 'windows' || OS === 'macos') && <SystemTools />}
      </div>
    </>
  );
}
