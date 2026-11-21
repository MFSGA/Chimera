import { createFileRoute } from '@tanstack/react-router';
import * as m from '@/paraglide/messages';
import LocalIpProbeSwitch from '../_modules/local-ip-probe-switch';
import IpLookupProviderList from '../_modules/public-ip/ip-lookup-provider-list';
import StunServerList from '../_modules/public-ip/stun-server-list';
import { SettingsGroup } from '../../_modules/settings-card';
import { SettingsTitle } from '../../_modules/settings-title';

export const Route = createFileRoute(
  '/(main)/main/settings/chimera/public-ip/',
)({
  component: RouteComponent,
});

function RouteComponent() {
  return (
    <>
      <SettingsTitle>{m.settings_label_public_ip()}</SettingsTitle>
      <div className="space-y-4 px-4 pb-4" data-slot="public-ip-settings">
        <SettingsGroup>
          <LocalIpProbeSwitch />
        </SettingsGroup>
        <StunServerList />
        <IpLookupProviderList />
      </div>
    </>
  );
}
