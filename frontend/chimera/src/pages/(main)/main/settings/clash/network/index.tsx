import { createFileRoute } from '@tanstack/react-router';
import { PageTitle } from '@/components/page-title';
import * as m from '@/paraglide/messages';
import AllowLanSwitch from '../_modules/allow-lan-switch';
import IPv6Switch from '../_modules/ipv6-switch';
import MipsCongestionControllerSelector from '../_modules/mips-congestion-controller-selector';
import MixedPortConfig from '../_modules/mixed-port-config';
import RandomPortSwitch from '../_modules/random-port-switch';
import TransparentProxyConfigSettings from '../_modules/transparent-proxy-config';
import TunStackSelector from '../_modules/tun-stack-selector';
import {
  SettingsCard,
  SettingsCardContent,
  SettingsGroup,
  SettingsLabel,
} from '../../_modules/settings-card';

export const Route = createFileRoute('/(main)/main/settings/clash/network/')({
  component: RouteComponent,
  staticData: {
    crumb: {
      title: () => m.settings_label_network(),
      parent: () => ({ to: '/main/settings' }),
    },
  },
});

const PortSettings = () => (
  <div data-slot="port-settings-container">
    <SettingsLabel>{m.settings_clash_settings_port_label()}</SettingsLabel>
    <SettingsGroup>
      <MixedPortConfig />
      <SettingsCard>
        <SettingsCardContent>
          <RandomPortSwitch />
        </SettingsCardContent>
      </SettingsCard>
    </SettingsGroup>
  </div>
);

const LanSettings = () => (
  <div data-slot="lan-settings-container">
    <SettingsLabel>{m.settings_label_network()}</SettingsLabel>
    <SettingsGroup>
      <SettingsCard>
        <SettingsCardContent>
          <AllowLanSwitch />
        </SettingsCardContent>
      </SettingsCard>
      <SettingsCard>
        <SettingsCardContent>
          <IPv6Switch />
        </SettingsCardContent>
      </SettingsCard>
    </SettingsGroup>
  </div>
);

function RouteComponent() {
  return (
    <>
      <PageTitle legacySlot="settings-title" />
      <div className="space-y-4 px-4 pb-4">
        <PortSettings />
        <TransparentProxyConfigSettings />
        <LanSettings />
        <TunStackSelector />
        <MipsCongestionControllerSelector />
      </div>
    </>
  );
}
